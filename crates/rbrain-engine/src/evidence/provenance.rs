//! `brain_provenance_of(slug)` — reverse-walk the research graph to answer
//! "what produced this result/artifact/finding?".
//!
//! Enumerates incoming and outgoing research edges for the queried page. This is
//! an adjacency report, not a recursive traversal; use `brain_evidence_check`
//! for a finding's resolved evidence chain.
//!
//! Result is shaped so ZeroClaw can directly render a human-readable trail:
//!
//! ```text
//! research/findings/x
//!   ↳ supports     → research/artifacts/desc-stats   (artifact)
//!       ↳ derived_from → research/datasets/cohort     (dataset)
//!       ↳ computed_by  → research/scripts/desc.py     (script)
//!   ⇐ produces from research/runs/cohort-q2          (research_run)
//! ```

use rbrain_core::error::{BrainError, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::research::edges::ALL as RESEARCH_EDGES;
use crate::research::TenantContext;

fn db_err<E: std::fmt::Display>(e: E) -> BrainError {
    BrainError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceEdge {
    /// Edge label from the SQLite links table.
    pub edge_type: String,
    /// The neighbouring slug — direction depends on `incoming`.
    pub neighbour_slug: String,
    pub neighbour_page_type: String,
    /// True if the edge points **into** the queried slug (`neighbour --edge--> slug`).
    /// False if it points away (`slug --edge--> neighbour`).
    pub incoming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceReport {
    pub slug: String,
    pub page_type: String,
    pub edges: Vec<ProvenanceEdge>,
}

fn numbered_placeholders(start: usize, n: usize) -> String {
    (start..start + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn readable_users(ctx: &TenantContext) -> Vec<String> {
    ctx.readable_user_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

pub async fn provenance_of(pool: &SqlitePool, slug: &str) -> Result<ProvenanceReport> {
    provenance_of_with_ctx(pool, slug, &TenantContext::default_tenant()).await
}

pub async fn provenance_of_with_ctx(
    pool: &SqlitePool,
    slug: &str,
    ctx: &TenantContext,
) -> Result<ProvenanceReport> {
    // Resolve the page type once so callers can render headings without a
    // second round-trip.
    let readable = readable_users(ctx);
    let user_ph = numbered_placeholders(2, readable.len());
    let sql = format!("SELECT page_type FROM pages WHERE slug = ?1 AND user_id IN ({user_ph})");
    let mut q = sqlx::query_scalar(&sql).bind(slug);
    for user_id in &readable {
        q = q.bind(user_id);
    }
    let page_type: String = q
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| BrainError::Conflict(format!("page not found: {slug}")))?;

    let edge_names: Vec<&str> = RESEARCH_EDGES.iter().map(|e| e.as_str()).collect();
    let ph = numbered_placeholders(2, edge_names.len());
    let user_ph = numbered_placeholders(2 + edge_names.len(), readable.len());

    // Outgoing edges: slug --edge--> neighbour
    let out_sql = format!(
        "SELECT l.edge_type, p.slug AS neighbour_slug, p.page_type AS neighbour_page_type
         FROM links l JOIN pages p ON p.slug = l.target_slug
         WHERE l.source_slug = ?1 AND l.edge_type IN ({ph}) AND l.user_id IN ({user_ph})
         ORDER BY l.edge_type, p.slug"
    );
    let mut q = sqlx::query(&out_sql).bind(slug);
    for e in &edge_names {
        q = q.bind(*e);
    }
    for user_id in &readable {
        q = q.bind(user_id);
    }
    let out_rows = q.fetch_all(pool).await.map_err(db_err)?;

    // Incoming edges: neighbour --edge--> slug
    let in_sql = format!(
        "SELECT l.edge_type, p.slug AS neighbour_slug, p.page_type AS neighbour_page_type
         FROM links l JOIN pages p ON p.slug = l.source_slug
         WHERE l.target_slug = ?1 AND l.edge_type IN ({ph}) AND l.user_id IN ({user_ph})
         ORDER BY l.edge_type, p.slug"
    );
    let mut q = sqlx::query(&in_sql).bind(slug);
    for e in &edge_names {
        q = q.bind(*e);
    }
    for user_id in &readable {
        q = q.bind(user_id);
    }
    let in_rows = q.fetch_all(pool).await.map_err(db_err)?;

    let mut edges: Vec<ProvenanceEdge> = Vec::new();
    for row in &out_rows {
        edges.push(ProvenanceEdge {
            edge_type: row.try_get("edge_type").map_err(db_err)?,
            neighbour_slug: row.try_get("neighbour_slug").map_err(db_err)?,
            neighbour_page_type: row.try_get("neighbour_page_type").map_err(db_err)?,
            incoming: false,
        });
    }
    for row in &in_rows {
        edges.push(ProvenanceEdge {
            edge_type: row.try_get("edge_type").map_err(db_err)?,
            neighbour_slug: row.try_get("neighbour_slug").map_err(db_err)?,
            neighbour_page_type: row.try_get("neighbour_page_type").map_err(db_err)?,
            incoming: true,
        });
    }

    Ok(ProvenanceReport {
        slug: slug.to_string(),
        page_type,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_one_per_edge() {
        assert_eq!(numbered_placeholders(1, 3), "?1,?2,?3");
        assert_eq!(numbered_placeholders(4, 1), "?4");
    }

    #[test]
    fn edge_whitelist_covers_phase3_set() {
        // Sanity-check: every Phase 3 research edge must appear in the source
        // vocabulary used by provenance_of, including contradiction edges.
        for e in [
            "derived_from",
            "computed_by",
            "uses_dataset",
            "uses_method",
            "uses_variable",
            "supports",
            "contradicts",
            "produces",
            "cites",
            "tests_hypothesis",
            "validates",
            "limits",
        ] {
            assert!(
                crate::research::edges::ALL
                    .iter()
                    .any(|edge| edge.as_str() == e),
                "whitelist missing {e}",
            );
        }
    }
}
