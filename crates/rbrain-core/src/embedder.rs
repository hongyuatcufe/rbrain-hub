use async_trait::async_trait;
use crate::error::Result;
use crate::vector_store::SparseVec;

#[async_trait]
pub trait Embedder: Send + Sync {
    fn dimension(&self) -> usize;
    async fn embed_one(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Embed a batch and return (dense, sparse) pairs.
    /// Default implementation returns empty sparse vecs; override for real sparse output.
    async fn embed_batch_dual(&self, texts: &[String]) -> Result<Vec<(Vec<f32>, SparseVec)>> {
        let dense = self.embed_batch(texts).await?;
        Ok(dense.into_iter().map(|d| (d, SparseVec::default())).collect())
    }

    fn verify_deterministic(&self) -> bool;
}
