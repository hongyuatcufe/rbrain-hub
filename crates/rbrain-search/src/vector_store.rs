use std::path::PathBuf;
use std::sync::Arc;

use arrow_array::{FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch, RecordBatchIterator};
use arrow_buffer::OffsetBuffer;
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use futures::TryStreamExt;
use lancedb::index::vector::IvfPqIndexBuilder;
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Table};

use rbrain_core::error::{BrainError, Result};
use rbrain_core::vector_store::{SparseVec, VectorStore};

const TABLE_NAME: &str = "chunks";

fn io_err(e: impl std::fmt::Display) -> BrainError {
    BrainError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

fn schema(dim: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new(
            "dense",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dim,
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

fn validate_items(items: &[(i64, Vec<f32>, SparseVec)], dim: usize) -> Result<()> {
    for (chunk_id, dense, sparse) in items {
        if dense.len() != dim {
            return Err(io_err(format!(
                "chunk {} dense vector has dim {}, expected {}",
                chunk_id,
                dense.len(),
                dim
            )));
        }
        if sparse.indices.len() != sparse.values.len() {
            return Err(io_err(format!(
                "chunk {} sparse vector has {} indices but {} values",
                chunk_id,
                sparse.indices.len(),
                sparse.values.len()
            )));
        }
    }
    Ok(())
}

pub struct LanceStore {
    table: Table,
    dim: usize,
}

impl std::fmt::Debug for LanceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanceStore")
            .field("dim", &self.dim)
            .finish_non_exhaustive()
    }
}

impl LanceStore {
    pub async fn new(lance_dir: PathBuf, dim: usize) -> Result<Self> {
        let uri = lance_dir.to_str().ok_or_else(|| io_err("invalid lance_dir path"))?;
        let db = connect(uri).execute().await.map_err(io_err)?;

        let table_names = db.table_names().execute().await.map_err(io_err)?;
        let table = if table_names.contains(&TABLE_NAME.to_string()) {
            db.open_table(TABLE_NAME).execute().await.map_err(io_err)?
        } else {
            // Seed with an empty batch to establish schema.
            let schema = schema(dim as i32);
            let empty = empty_batch(dim as i32, &schema);
            let reader = RecordBatchIterator::new(vec![Ok(empty)], schema);
            db.create_table(TABLE_NAME, Box::new(reader))
                .execute()
                .await
                .map_err(io_err)?
        };

        Ok(Self { table, dim })
    }

    fn schema(&self) -> Arc<Schema> {
        schema(self.dim as i32)
    }
}

fn empty_batch(dim: i32, schema: &Arc<Schema>) -> RecordBatch {
    let ids = Int64Array::from(vec![] as Vec<i64>);
    let dense_values = Float32Array::from(vec![] as Vec<f32>);
    let dense = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        dim,
        Arc::new(dense_values),
        None,
    )
    .unwrap();
    let sparse_idx_col = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Int64, true)),
        OffsetBuffer::new(vec![0i32].into()),
        Arc::new(Int64Array::from(vec![] as Vec<i64>)),
        None,
    )
    .unwrap();
    let sparse_val_col = ListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        OffsetBuffer::new(vec![0i32].into()),
        Arc::new(Float32Array::from(vec![] as Vec<f32>)),
        None,
    )
    .unwrap();
    RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(ids),
            Arc::new(dense),
            Arc::new(sparse_idx_col),
            Arc::new(sparse_val_col),
        ],
    )
    .unwrap()
}

#[async_trait]
impl VectorStore for LanceStore {
    async fn upsert_batch(&self, items: &[(i64, Vec<f32>, SparseVec)]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        validate_items(items, self.dim)?;

        // Delete any existing entries for these IDs (true upsert semantics).
        let id_list: String = items
            .iter()
            .map(|(id, _, _)| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.table
            .delete(&format!("id IN ({})", id_list))
            .await
            .map_err(io_err)?;

        let schema = self.schema();
        let dim = self.dim as i32;

        let ids: Int64Array = items.iter().map(|(id, _, _)| *id).collect();

        // dense: FixedSizeList — all vectors concatenated, stride = dim
        let all_dense: Float32Array = items
            .iter()
            .flat_map(|(_, d, _)| d.iter().copied())
            .collect();
        let dense = FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            dim,
            Arc::new(all_dense),
            None,
        )
        .map_err(io_err)?;

        // sparse indices: variable-length List<i64>
        let mut s_idx_vals: Vec<i64> = Vec::new();
        let mut s_idx_offsets: Vec<i32> = vec![0];
        let mut s_val_vals: Vec<f32> = Vec::new();
        let mut s_val_offsets: Vec<i32> = vec![0];

        for (_, _, sparse) in items {
            s_idx_vals.extend(sparse.indices.iter().map(|&i| i as i64));
            s_idx_offsets.push(s_idx_vals.len() as i32);
            s_val_vals.extend(&sparse.values);
            s_val_offsets.push(s_val_vals.len() as i32);
        }

        let sparse_idx_col = ListArray::try_new(
            Arc::new(Field::new("item", DataType::Int64, true)),
            OffsetBuffer::new(s_idx_offsets.into()),
            Arc::new(Int64Array::from(s_idx_vals)),
            None,
        )
        .map_err(io_err)?;

        let sparse_val_col = ListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            OffsetBuffer::new(s_val_offsets.into()),
            Arc::new(Float32Array::from(s_val_vals)),
            None,
        )
        .map_err(io_err)?;

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(ids),
                Arc::new(dense),
                Arc::new(sparse_idx_col),
                Arc::new(sparse_val_col),
            ],
        )
        .map_err(io_err)?;

        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.table
            .add(Box::new(reader))
            .execute()
            .await
            .map_err(io_err)?;

        // Rebuild IVF-PQ index once the table grows large enough.
        let row_count = self.table.count_rows(None).await.unwrap_or(0);
        if row_count >= 256 {
            let _ = self
                .table
                .create_index(&["dense"], Index::IvfPq(IvfPqIndexBuilder::default()))
                .execute()
                .await;
        }

        Ok(())
    }

    async fn delete(&self, chunk_id: i64) -> Result<()> {
        self.table
            .delete(&format!("id = {}", chunk_id))
            .await
            .map_err(io_err)
    }

    async fn search_dense(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        if k == 0 {
            return Ok(Vec::new());
        }
        if query.len() != self.dim {
            return Err(io_err(format!(
                "query dense vector has dim {}, expected {}",
                query.len(),
                self.dim
            )));
        }

        let stream = self
            .table
            .query()
            .nearest_to(query)
            .map_err(io_err)?
            .limit(k)
            .execute()
            .await
            .map_err(io_err)?;
        let batches: Vec<RecordBatch> = stream.try_collect::<Vec<RecordBatch>>().await.map_err(io_err)?;

        let mut results = Vec::new();
        for batch in &batches {
            let id_col = batch
                .column_by_name("id")
                .and_then(|c| c.as_any().downcast_ref::<Int64Array>())
                .ok_or_else(|| io_err("missing 'id' column in LanceDB result"))?;

            // LanceDB returns _distance column for ANN results
            let dist_col = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

            for i in 0..batch.num_rows() {
                let chunk_id = id_col.value(i);
                let dist = dist_col.map(|c| c.value(i)).unwrap_or(0.0);
                results.push((chunk_id, dist));
            }
        }

        Ok(results)
    }

    async fn search_sparse(&self, _query: &SparseVec, _k: usize) -> Result<Vec<(i64, f32)>> {
        // LanceDB Rust SDK 0.17 does not yet expose sparse ANN search.
        // Return empty — hybrid_search gracefully skips empty lists in RRF.
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_items_accepts_matching_dense_and_sparse_lengths() {
        let items = vec![(
            1,
            vec![0.1, 0.2, 0.3],
            SparseVec {
                indices: vec![7, 11],
                values: vec![0.4, 0.5],
            },
        )];

        assert!(validate_items(&items, 3).is_ok());
    }

    #[test]
    fn validate_items_rejects_wrong_dense_dim() {
        let items = vec![(1, vec![0.1, 0.2], SparseVec::default())];

        let err = validate_items(&items, 3).unwrap_err().to_string();
        assert!(err.contains("dense vector has dim 2, expected 3"));
    }

    #[test]
    fn validate_items_rejects_sparse_length_mismatch() {
        let items = vec![(
            1,
            vec![0.1, 0.2, 0.3],
            SparseVec {
                indices: vec![7, 11],
                values: vec![0.4],
            },
        )];

        let err = validate_items(&items, 3).unwrap_err().to_string();
        assert!(err.contains("sparse vector has 2 indices but 1 values"));
    }
}
