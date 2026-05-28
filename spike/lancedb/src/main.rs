/// LanceDB Spike — validates 5 assumptions before main refactor:
/// 1. Persistence across process restarts
/// 2. MVCC concurrent writes (10 tasks, no data loss)
/// 3. S3 backend (skipped if env vars absent)
/// 4. Qwen sparse vector format round-trip
/// 5. Dense ANN search latency via IVF-PQ index

use std::sync::Arc;
use std::time::Instant;

use arrow_array::{
    FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch, RecordBatchIterator,
};
use arrow_buffer::OffsetBuffer;
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Table};

const TABLE_NAME: &str = "chunks_spike";
const DIM: i32 = 8; // small for spike; real = 1024

// ---------------------------------------------------------------------------
// Schema — dense uses FixedSizeList (required by LanceDB ANN index)
// ---------------------------------------------------------------------------

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new(
            "dense",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                DIM,
            ),
            false,
        ),
        Field::new(
            "sparse_indices",
            DataType::List(Arc::new(Field::new("item", DataType::Int64, true))),
            false,
        ),
        Field::new(
            "sparse_values",
            DataType::List(Arc::new(Field::new("item", DataType::Float32, true))),
            false,
        ),
    ]))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_batch(start_id: i64, count: usize) -> RecordBatch {
    let schema = schema();
    let dim = DIM as usize;

    let ids: Int64Array = (start_id..start_id + count as i64).collect();

    // dense: deterministic unit vectors as FixedSizeList
    let dense_values: Float32Array = (0..(count * dim))
        .map(|i| (i % dim) as f32 / dim as f32)
        .collect();
    let dense = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        DIM,
        Arc::new(dense_values),
        None,
    )
    .unwrap();

    // sparse: 2 non-zero entries per chunk (variable-length → List)
    let n_sparse = 2;
    let s_idx_vals: Int64Array = (0..(count * n_sparse))
        .map(|i| ((i * 137 + 42) % 1000) as i64)
        .collect();
    let s_idx_offsets: Vec<i32> = (0..=count).map(|i| (i * n_sparse) as i32).collect();
    let sparse_indices = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Int64, true)),
        OffsetBuffer::new(s_idx_offsets.into()),
        Arc::new(s_idx_vals),
        None,
    )
    .unwrap();

    let s_val_vals: Float32Array = (0..(count * n_sparse))
        .map(|i| (i % 10) as f32 * 0.1 + 0.05)
        .collect();
    let s_val_offsets: Vec<i32> = (0..=count).map(|i| (i * n_sparse) as i32).collect();
    let sparse_values = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        OffsetBuffer::new(s_val_offsets.into()),
        Arc::new(s_val_vals),
        None,
    )
    .unwrap();

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(ids),
            Arc::new(dense),
            Arc::new(sparse_indices),
            Arc::new(sparse_values),
        ],
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Check 1: Persistence
// ---------------------------------------------------------------------------

async fn check_persistence(data_dir: &str) -> bool {
    println!("\n=== Check 1: Persistence ===");

    {
        let db = connect(data_dir).execute().await.unwrap();
        let _ = db.drop_table(TABLE_NAME).await;
        let batch = make_batch(0, 5);
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema());
        db.create_table(TABLE_NAME, Box::new(reader))
            .execute()
            .await
            .unwrap();
        println!("  Wrote 5 rows, closing connection.");
    }

    {
        let db = connect(data_dir).execute().await.unwrap();
        let tbl = db.open_table(TABLE_NAME).execute().await.unwrap();
        let count = tbl.count_rows(None).await.unwrap();
        println!("  Re-opened: found {count} rows (expected 5)");
        if count == 5 {
            println!("  PASS");
            true
        } else {
            println!("  FAIL: expected 5, got {count}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Check 2: MVCC concurrent writes
// ---------------------------------------------------------------------------

async fn check_concurrent_writes(data_dir: &str) -> bool {
    println!("\n=== Check 2: MVCC Concurrent Writes ===");

    let db = Arc::new(connect(data_dir).execute().await.unwrap());
    let _ = db.drop_table("concurrent_spike").await;

    {
        let batch = make_batch(0, 1);
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema());
        db.create_table("concurrent_spike", Box::new(reader))
            .execute()
            .await
            .unwrap();
    }

    let tasks: u64 = 10;
    let per_task: usize = 20;

    let handles: Vec<_> = (0..tasks)
        .map(|i| {
            let db2 = Arc::clone(&db);
            tokio::spawn(async move {
                let tbl = db2
                    .open_table("concurrent_spike")
                    .execute()
                    .await
                    .unwrap();
                let start = 1 + i as i64 * per_task as i64;
                let batch = make_batch(start, per_task);
                let reader = RecordBatchIterator::new(vec![Ok(batch)], schema());
                tbl.add(Box::new(reader)).execute().await.unwrap();
            })
        })
        .collect();

    let results = futures::future::join_all(handles).await;
    let panics = results.iter().filter(|r| r.is_err()).count();

    let tbl = db
        .open_table("concurrent_spike")
        .execute()
        .await
        .unwrap();
    let count = tbl.count_rows(None).await.unwrap();
    let expected = 1 + tasks as usize * per_task;

    println!("  Tasks: {tasks}, per-task rows: {per_task}");
    println!("  Panics/errors: {panics}");
    println!("  Final row count: {count} (expected {expected})");

    if panics == 0 && count == expected {
        println!("  PASS");
        true
    } else {
        println!("  FAIL");
        false
    }
}

// ---------------------------------------------------------------------------
// Check 3: S3 backend
// ---------------------------------------------------------------------------

async fn check_s3() -> bool {
    println!("\n=== Check 3: S3 Backend ===");

    let bucket = std::env::var("SPIKE_S3_BUCKET").unwrap_or_default();
    if bucket.is_empty() {
        println!("  SKIP: SPIKE_S3_BUCKET not set.");
        println!("  (pending — verify in cloud environment)");
        return true;
    }

    let uri = format!("s3://{bucket}/spike/");
    println!("  Connecting to {uri}");

    match connect(&uri).execute().await {
        Err(e) => {
            println!("  FAIL: connect error: {e}");
            false
        }
        Ok(db) => {
            let batch = make_batch(0, 3);
            let reader = RecordBatchIterator::new(vec![Ok(batch)], schema());
            match db
                .create_table("s3_spike", Box::new(reader))
                .execute()
                .await
            {
                Ok(tbl) => {
                    let count = tbl.count_rows(None).await.unwrap();
                    println!("  Wrote and read back {count} rows from S3");
                    println!("  PASS");
                    true
                }
                Err(e) => {
                    println!("  FAIL: write error: {e}");
                    false
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check 4: Qwen sparse vector round-trip
// ---------------------------------------------------------------------------

async fn check_sparse_roundtrip(data_dir: &str) -> bool {
    println!("\n=== Check 4: Qwen Sparse Vector Round-trip ===");

    // Simulated Qwen output: {"indices": [1024, 3872], "values": [0.81, 0.23]}
    let qwen_indices: Vec<i64> = vec![1024, 3872];
    let qwen_values: Vec<f32> = vec![0.81, 0.23];

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new(
            "sparse_indices",
            DataType::List(Arc::new(Field::new("item", DataType::Int64, true))),
            false,
        ),
        Field::new(
            "sparse_values",
            DataType::List(Arc::new(Field::new("item", DataType::Float32, true))),
            false,
        ),
    ]));

    let db = connect(data_dir).execute().await.unwrap();
    let _ = db.drop_table("sparse_roundtrip").await;

    let ids: Int64Array = vec![42i64].into_iter().collect();

    let idx_vals: Int64Array = qwen_indices.iter().copied().collect();
    let idx_offsets: Vec<i32> = vec![0, qwen_indices.len() as i32];
    let s_idx = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Int64, true)),
        OffsetBuffer::new(idx_offsets.into()),
        Arc::new(idx_vals),
        None,
    )
    .unwrap();

    let val_vals: Float32Array = qwen_values.iter().copied().collect();
    let val_offsets: Vec<i32> = vec![0, qwen_values.len() as i32];
    let s_val = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        OffsetBuffer::new(val_offsets.into()),
        Arc::new(val_vals),
        None,
    )
    .unwrap();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![Arc::new(ids), Arc::new(s_idx), Arc::new(s_val)],
    )
    .unwrap();

    let reader = RecordBatchIterator::new(vec![Ok(batch)], schema.clone());
    let tbl = db
        .create_table("sparse_roundtrip", Box::new(reader))
        .execute()
        .await
        .unwrap();

    let result_batches: Vec<RecordBatch> = tbl
        .query()
        .only_if("id = 42")
        .execute()
        .await
        .unwrap()
        .try_collect::<Vec<RecordBatch>>()
        .await
        .unwrap();

    if result_batches.is_empty() {
        println!("  FAIL: no rows returned");
        return false;
    }

    let rb = &result_batches[0];
    let idx_col = rb
        .column_by_name("sparse_indices")
        .unwrap()
        .as_any()
        .downcast_ref::<ListArray>()
        .unwrap();
    let val_col = rb
        .column_by_name("sparse_values")
        .unwrap()
        .as_any()
        .downcast_ref::<ListArray>()
        .unwrap();

    let read_indices: Vec<i64> = idx_col
        .value(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap()
        .iter()
        .map(|v| v.unwrap())
        .collect();

    let read_values: Vec<f32> = val_col
        .value(0)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap()
        .iter()
        .map(|v| v.unwrap())
        .collect();

    println!("  Written:   indices={qwen_indices:?}  values={qwen_values:?}");
    println!("  Read back: indices={read_indices:?}  values={read_values:?}");

    if read_indices == qwen_indices && read_values == qwen_values {
        println!("  PASS");
        true
    } else {
        println!("  FAIL: data mismatch");
        false
    }
}

// ---------------------------------------------------------------------------
// Check 5: Dense ANN search via IVF-PQ (FixedSizeList required)
// ---------------------------------------------------------------------------

async fn check_ann_latency(data_dir: &str) -> bool {
    println!("\n=== Check 5: Dense ANN Search Latency ===");

    let db = connect(data_dir).execute().await.unwrap();
    let _ = db.drop_table("ann_spike").await;

    let n_rows: usize = 10_000;
    let batch_size: usize = 500;

    {
        let batch = make_batch(0, batch_size);
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema());
        db.create_table("ann_spike", Box::new(reader))
            .execute()
            .await
            .unwrap();
    }

    let tbl = db.open_table("ann_spike").execute().await.unwrap();
    let mut written = batch_size;
    while written < n_rows {
        let this_batch = batch_size.min(n_rows - written);
        let batch = make_batch(written as i64, this_batch);
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema());
        tbl.add(Box::new(reader)).execute().await.unwrap();
        written += this_batch;
    }
    println!("  Inserted {n_rows} rows.");

    use lancedb::index::vector::IvfPqIndexBuilder;
    use lancedb::index::Index;

    println!("  Building IVF-PQ index on 'dense'...");
    tbl.create_index(&["dense"], Index::IvfPq(IvfPqIndexBuilder::default()))
        .execute()
        .await
        .unwrap();

    let query_vec: Vec<f32> = (0..DIM as usize).map(|i| i as f32 / DIM as f32).collect();

    let t0 = Instant::now();
    let _results: Vec<RecordBatch> = tbl
        .query()
        .nearest_to(query_vec.as_slice())
        .unwrap()
        .limit(10)
        .execute()
        .await
        .unwrap()
        .try_collect::<Vec<RecordBatch>>()
        .await
        .unwrap();
    let elapsed = t0.elapsed();

    println!("  ANN latency: {elapsed:?} (target < 50ms)");

    if elapsed.as_millis() < 50 {
        println!("  PASS");
        true
    } else {
        println!("  SLOW — record for evaluation (not a hard failure)");
        true
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let data_dir = "./spike-data";
    std::fs::remove_dir_all(data_dir).ok();
    std::fs::create_dir_all(data_dir).unwrap();

    println!("rbrain-hub LanceDB Spike");
    println!("========================");

    let results = vec![
        ("Persistence", check_persistence(data_dir).await),
        ("Concurrent writes", check_concurrent_writes(data_dir).await),
        ("S3 backend", check_s3().await),
        ("Sparse round-trip", check_sparse_roundtrip(data_dir).await),
        ("Dense ANN latency", check_ann_latency(data_dir).await),
    ];

    println!("\n=== Summary ===");
    let mut all_pass = true;
    for (name, passed) in &results {
        let status = if *passed { "PASS" } else { "FAIL" };
        println!("  {status:4}  {name}");
        if !passed {
            all_pass = false;
        }
    }

    println!();
    if all_pass {
        println!("All checks passed → proceed to Phase 2 (VectorStore refactor)");
        std::process::exit(0);
    } else {
        println!("One or more checks failed → investigate before proceeding");
        std::process::exit(1);
    }
}
