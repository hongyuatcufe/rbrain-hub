use async_trait::async_trait;
use crate::error::Result;

/// Sparse vector in (indices, values) coordinate format,
/// as returned by Qwen text-embedding-v4 with output_type="dense&sparse".
#[derive(Debug, Clone, Default)]
pub struct SparseVec {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Upsert a batch of (chunk_id, dense_vec, sparse_vec) into the store.
    /// Replaces any existing entry for each chunk_id.
    async fn upsert_batch(&self, items: &[(i64, Vec<f32>, SparseVec)]) -> Result<()>;

    /// Remove a single chunk from the store.
    async fn delete(&self, chunk_id: i64) -> Result<()>;

    /// Dense ANN search — returns (chunk_id, cosine_distance) pairs.
    async fn search_dense(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>>;

    /// Sparse ANN search — returns (chunk_id, score) pairs.
    /// Returns empty vec when the backend does not support sparse ANN yet.
    async fn search_sparse(&self, query: &SparseVec, k: usize) -> Result<Vec<(i64, f32)>>;
}
