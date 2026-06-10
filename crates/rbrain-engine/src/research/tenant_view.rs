//! [`TenantView`] — a thin facade over [`Engine`] that carries a
//! [`TenantContext`] for the duration of a chain of calls.
//!
//! Compared to threading `&TenantContext` through every public Engine
//! method, this lets call sites read like:
//!
//! ```ignore
//! let view = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
//! view.put_page(page).await?;
//! let p = view.get_page("research/findings/x").await?;
//! ```
//!
//! All methods on the view dispatch to the underlying Engine's
//! `*_with_ctx` variants, so the tenant filtering is enforced in one
//! place (the Engine SQL) rather than scattered across callers.

use rbrain_core::error::Result;
use rbrain_core::page::Page;

use crate::engine::{BrainStats, ChunkResult, Engine, GraphEdge};
use crate::links::LinkRef;

use super::tenant::TenantContext;

/// Borrowed view over an [`Engine`] bound to a specific [`TenantContext`].
///
/// The view holds an `&Engine` reference and the context. All read/write
/// methods delegate to the tenant-aware Engine methods.
#[derive(Debug)]
pub struct TenantView<'a> {
    engine: &'a Engine,
    ctx: TenantContext,
}

impl<'a> TenantView<'a> {
    /// Construct directly. Prefer [`Engine::with_tenant`].
    pub fn new(engine: &'a Engine, ctx: TenantContext) -> Self {
        Self { engine, ctx }
    }

    /// The underlying engine — useful for methods that haven't yet been
    /// surfaced on the view, or for admin/system operations that bypass
    /// tenancy entirely.
    pub fn engine(&self) -> &Engine {
        self.engine
    }

    pub fn ctx(&self) -> &TenantContext {
        &self.ctx
    }

    // ── Pages ──────────────────────────────────────────────────────────

    pub async fn put_page(&self, page: Page) -> Result<()> {
        self.engine.put_page_with_ctx(page, &self.ctx).await
    }

    pub async fn put_page_force(&self, page: Page) -> Result<()> {
        self.engine.put_page_force_with_ctx(page, &self.ctx).await
    }

    pub async fn get_page(&self, slug: &str) -> Result<Page> {
        self.engine.get_page_with_ctx(slug, &self.ctx).await
    }

    pub async fn delete_page(&self, slug: &str) -> Result<()> {
        self.engine.delete_page_with_ctx(slug, &self.ctx).await
    }

    pub async fn list_pages(
        &self,
        page_type: Option<&str>,
        tag: Option<&str>,
        language: Option<&str>,
        limit: Option<i64>,
        sort_by: Option<&str>,
    ) -> Result<Vec<Page>> {
        self.engine
            .list_pages_with_ctx(page_type, tag, language, limit, sort_by, &self.ctx)
            .await
    }

    // ── Links / graph ──────────────────────────────────────────────────

    pub async fn add_link(
        &self,
        source_slug: &str,
        target_slug: &str,
        edge_type: &str,
        context: Option<&str>,
        chunk_id: Option<i64>,
    ) -> Result<()> {
        self.engine
            .add_link_with_ctx(source_slug, target_slug, edge_type, context, chunk_id, &self.ctx)
            .await
    }

    pub async fn outlinks(&self, slug: &str) -> Result<Vec<LinkRef>> {
        self.engine.outlinks_with_ctx(slug, &self.ctx).await
    }

    pub async fn backlinks(&self, slug: &str) -> Result<Vec<LinkRef>> {
        self.engine.backlinks_with_ctx(slug, &self.ctx).await
    }

    pub async fn graph_query(
        &self,
        slug: &str,
        edge_type: Option<&str>,
        depth: usize,
        direction: &str,
    ) -> Result<Vec<GraphEdge>> {
        self.engine
            .graph_query_with_ctx(slug, edge_type, depth, direction, &self.ctx)
            .await
    }

    pub async fn search_with_context(
        &self,
        query: &str,
        lang: &rbrain_core::page::Language,
        k: usize,
        expand: bool,
        max_chunks_per_page: usize,
    ) -> Result<Vec<ChunkResult>> {
        self.engine
            .search_with_context_with_ctx(query, lang, k, expand, max_chunks_per_page, &self.ctx)
            .await
    }

    pub async fn get_stats(&self) -> Result<BrainStats> {
        self.engine.get_stats_with_ctx(&self.ctx).await
    }
}
