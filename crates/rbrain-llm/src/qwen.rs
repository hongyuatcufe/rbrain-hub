use async_trait::async_trait;
use rbrain_core::config::QwenConfig;
use rbrain_core::embedder::Embedder;
use rbrain_core::error::{BrainError, Result};
use rbrain_core::vector_store::SparseVec;
use serde::Deserialize;
use std::env;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const DEFAULT_MODEL: &str = "text-embedding-v4";
const DEFAULT_DIM: usize = 1024;

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    sparse_embedding: Option<SparseEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct SparseEmbeddingData {
    indices: Vec<u32>,
    values: Vec<f32>,
}

#[derive(Debug)]
pub struct QwenEmbedder {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    dimension: usize,
}

impl QwenEmbedder {
    pub fn from_config(cfg: &QwenConfig) -> Result<Self> {
        let api_key = if !cfg.api_key.is_empty() {
            cfg.api_key.clone()
        } else {
            env::var("DASHSCOPE_API_KEY").map_err(|_| BrainError::ApiUnreachable {
                provider: "qwen".to_string(),
                message: "DASHSCOPE_API_KEY not set and config.qwen.api_key is empty".to_string(),
            })?
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| BrainError::ApiUnreachable {
                provider: "qwen".to_string(),
                message: e.to_string(),
            })?;

        Ok(Self {
            client,
            api_key,
            base_url: cfg.base_url.clone(),
            model: cfg.model.clone(),
            dimension: DEFAULT_DIM,
        })
    }

    pub fn new(base_url: Option<&str>, model: Option<&str>, dim: Option<usize>) -> Result<Self> {
        let api_key = env::var("DASHSCOPE_API_KEY")
            .map_err(|_| BrainError::ApiUnreachable {
                provider: "qwen".to_string(),
                message: "DASHSCOPE_API_KEY not set".to_string(),
            })?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| BrainError::ApiUnreachable {
                provider: "qwen".to_string(),
                message: e.to_string(),
            })?;

        Ok(Self {
            client,
            api_key,
            base_url: base_url.unwrap_or(DEFAULT_BASE_URL).to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            dimension: dim.unwrap_or(DEFAULT_DIM),
        })
    }

    async fn call_embeddings_api(
        &self,
        texts: &[String],
        output_type: &str,
    ) -> Result<Vec<EmbeddingData>> {
        let mut all_data: Vec<EmbeddingData> = Vec::new();
        let batch_size = 10;

        for chunk in texts.chunks(batch_size) {
            let resp = self
                .client
                .post(format!("{}/embeddings", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "model": self.model,
                    "input": chunk,
                    "output_type": output_type,
                }))
                .send()
                .await
                .map_err(|e| BrainError::ApiUnreachable {
                    provider: "qwen".to_string(),
                    message: e.to_string(),
                })?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(BrainError::ApiUnreachable {
                    provider: "qwen".to_string(),
                    message: format!("HTTP {}: {}", status, body),
                });
            }

            let response: EmbeddingResponse =
                resp.json().await.map_err(|e| BrainError::ApiUnreachable {
                    provider: "qwen".to_string(),
                    message: format!("Failed to parse response: {}", e),
                })?;

            all_data.extend(response.data);
        }

        Ok(all_data)
    }
}

#[async_trait]
impl Embedder for QwenEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut result = self.embed_batch(&[text.to_string()]).await?;
        result.pop().ok_or_else(|| BrainError::ApiUnreachable {
            provider: "qwen".to_string(),
            message: "Empty response".to_string(),
        })
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let data = self.call_embeddings_api(texts, "dense").await?;
        Ok(data.into_iter().map(|d| d.embedding).collect())
    }

    /// Call Qwen with output_type="dense&sparse" to get both vectors in one API call.
    async fn embed_batch_dual(&self, texts: &[String]) -> Result<Vec<(Vec<f32>, SparseVec)>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let data = self.call_embeddings_api(texts, "dense&sparse").await?;
        Ok(data
            .into_iter()
            .map(|d| {
                let sparse = d.sparse_embedding.map_or(SparseVec::default(), |s| SparseVec {
                    indices: s.indices,
                    values: s.values,
                });
                (d.embedding, sparse)
            })
            .collect())
    }

    fn verify_deterministic(&self) -> bool {
        false
    }
}
