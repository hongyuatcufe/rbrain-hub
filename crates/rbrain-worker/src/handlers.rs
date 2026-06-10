use crate::{Job, JobHandler};
use async_trait::async_trait;
use rbrain_core::error::Result;
use rbrain_engine::{Engine, TenantContext};
use tracing::info;

pub struct EmbedPageHandler {
    engine: Engine,
}

impl std::fmt::Debug for EmbedPageHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedPageHandler").finish_non_exhaustive()
    }
}

impl EmbedPageHandler {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl JobHandler for EmbedPageHandler {
    fn name(&self) -> &str {
        "embed_page"
    }

    async fn handle(
        &self,
        _job: &Job,
        params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let slug = params
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or_else(|| rbrain_core::error::BrainError::Conflict(
                "embed_page job requires 'slug' parameter".to_string()
            ))?;

        info!("Processing embed_page job for slug: {}", slug);

        let ctx = tenant_from_params(&params);
        let page = self.engine.get_page_with_ctx(slug, &ctx).await?;

        self.engine.chunk_and_embed_page_with_ctx(&page, &ctx).await?;

        info!("Successfully embedded page: {}", slug);

        Ok(Some(serde_json::json!({ "slug": slug, "status": "embedded" })))
    }
}

fn tenant_from_params(params: &serde_json::Value) -> TenantContext {
    let user_id = params
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    match user_id {
        "global" => TenantContext::global(),
        "admin" => TenantContext::admin(),
        "default" => TenantContext::default_tenant(),
        _ => TenantContext::for_user(user_id, project_id),
    }
}

pub struct SyncRepoHandler {
    engine: Engine,
}

impl std::fmt::Debug for SyncRepoHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncRepoHandler").finish_non_exhaustive()
    }
}

impl SyncRepoHandler {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl JobHandler for SyncRepoHandler {
    fn name(&self) -> &str {
        "sync_repo"
    }

    async fn handle(
        &self,
        _job: &Job,
        _params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        info!("Processing sync_repo job");

        let (imported, updated, orphaned) = self.engine.sync().await?;

        info!(
            "Sync complete: {} imported, {} updated, {} orphaned",
            imported.len(),
            updated.len(),
            orphaned.len()
        );

        Ok(Some(serde_json::json!({
            "imported": imported,
            "updated": updated,
            "orphaned": orphaned
        })))
    }
}

pub struct ExtractLinksHandler {
    engine: Engine,
}

impl std::fmt::Debug for ExtractLinksHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractLinksHandler").finish_non_exhaustive()
    }
}

impl ExtractLinksHandler {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl JobHandler for ExtractLinksHandler {
    fn name(&self) -> &str {
        "extract_links"
    }

    async fn handle(
        &self,
        _job: &Job,
        params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let slug = params
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or_else(|| rbrain_core::error::BrainError::Conflict(
                "extract_links job requires 'slug' parameter".to_string()
            ))?;

        info!("Processing extract_links job for slug: {}", slug);

        let page = self.engine.get_page(slug).await?;
        let full_content = format!("{} {}", page.compiled_truth, page.timeline);
        let links = rbrain_engine::extract_links(&full_content);

        let extracted: Vec<serde_json::Value> = links
            .iter()
            .map(|link| {
                serde_json::json!({
                    "target_slug": link.target_slug,
                    "edge_type": link.edge_type,
                    "context": link.context
                })
            })
            .collect();

        info!("Extracted {} links from page: {}", extracted.len(), slug);

        Ok(Some(serde_json::json!({
            "slug": slug,
            "links_count": extracted.len(),
            "links": extracted
        })))
    }
}
