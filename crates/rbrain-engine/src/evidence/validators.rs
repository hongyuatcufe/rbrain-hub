//! Validators for both data-analysis and literature-review profiles.
//!
//! Each validator inspects DB state without executing analysis code
//! (CLAUDE.md §5). Data-analysis validators are scoped to a `run_slug`;
//! literature-review validators (M3 Slice 1) currently scan globally
//! by page_type — the literature_review pipeline does not yet emit
//! `produces` edges from a `research_run` to its outputs. M3 Slice 3
//! will add per-run scoping.

use rbrain_core::error::{BrainError, Result};
use sqlx::{Row, SqlitePool};

use super::actions::SuggestedAction;
use super::result::ValidatorResult;
use crate::links::extract_links;

fn db_err<E: std::fmt::Display>(e: E) -> BrainError {
    BrainError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    ))
}

/// Slug of the research_run page; used as the link source for `uses_dataset`
/// and as the link target for `produces` etc.
pub async fn dataset_registered(pool: &SqlitePool, run_slug: &str) -> Result<ValidatorResult> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM links l
         JOIN pages p ON p.slug = l.target_slug
         WHERE l.source_slug = ? AND l.edge_type = 'uses_dataset'
               AND p.page_type = 'dataset'",
    )
    .bind(run_slug)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    Ok(if count > 0 {
        ValidatorResult::pass("dataset_registered")
    } else {
        ValidatorResult::fail(
            "dataset_registered",
            "research_run has no uses_dataset → dataset link",
        )
        .with_affected([run_slug.to_string()])
        .with_actions([SuggestedAction::RegisterDataset { hint: None }])
    })
}

pub async fn artifact_hash_present(pool: &SqlitePool, run_slug: &str) -> Result<ValidatorResult> {
    // All artifact pages linked from this run via any edge.
    let rows = sqlx::query(
        "SELECT DISTINCT p.slug, p.frontmatter
         FROM links l
         JOIN pages p ON p.slug = l.target_slug
         WHERE l.source_slug = ? AND p.page_type = 'artifact'",
    )
    .bind(run_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    if rows.is_empty() {
        // No artifacts yet — warn, not fail. Run may still be in `register_dataset`.
        return Ok(ValidatorResult::warn(
            "artifact_hash_present",
            "no artifacts linked from research_run yet",
        ));
    }

    let mut missing: Vec<String> = Vec::new();
    for row in &rows {
        let slug: String = row.try_get("slug").map_err(db_err)?;
        let fm: String = row.try_get("frontmatter").map_err(db_err)?;
        let fm: serde_json::Value = serde_json::from_str(&fm).unwrap_or(serde_json::Value::Null);
        let hash = fm.get("hash").and_then(|v| v.as_str()).unwrap_or("");
        if hash.is_empty() {
            missing.push(slug);
        }
    }

    Ok(if missing.is_empty() {
        ValidatorResult::pass("artifact_hash_present")
    } else {
        let actions: Vec<SuggestedAction> = missing
            .iter()
            .map(|s| SuggestedAction::RegisterArtifact {
                hint: Some(s.clone()),
            })
            .collect();
        ValidatorResult::fail(
            "artifact_hash_present",
            format!("{} artifact(s) missing hash", missing.len()),
        )
        .with_affected(missing.clone())
        .with_actions(actions)
    })
}

pub async fn analysis_plan_exists(pool: &SqlitePool, run_slug: &str) -> Result<ValidatorResult> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM links l
         JOIN pages p ON p.slug = l.target_slug
         WHERE l.source_slug = ?
           AND l.edge_type = 'produces'
           AND p.page_type = 'analysis_plan'",
    )
    .bind(run_slug)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    Ok(if count > 0 {
        ValidatorResult::pass("analysis_plan_exists")
    } else {
        ValidatorResult::fail(
            "analysis_plan_exists",
            "research_run has no linked analysis_plan",
        )
        .with_affected([run_slug.to_string()])
        .with_actions([SuggestedAction::RecordAnalysisPlan {
            run_slug: run_slug.to_string(),
        }])
    })
}

pub async fn finding_has_supporting_artifact(
    pool: &SqlitePool,
    run_slug: &str,
) -> Result<ValidatorResult> {
    // Only findings this run produced directly (run --produces--> finding).
    let rows = sqlx::query(
        "SELECT DISTINCT p.slug, p.frontmatter FROM pages p
         JOIN links l ON l.target_slug = p.slug
         WHERE p.page_type = 'finding'
           AND l.source_slug = ?1
           AND l.edge_type = 'produces'",
    )
    .bind(run_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    if rows.is_empty() {
        return Ok(ValidatorResult::warn(
            "finding_has_supporting_artifact",
            "no finding linked to research_run yet",
        ));
    }

    let mut warn_drafts: Vec<String> = Vec::new();
    let mut fail_claims: Vec<String> = Vec::new();

    for row in &rows {
        let slug: String = row.try_get("slug").map_err(db_err)?;
        let fm: String = row.try_get("frontmatter").map_err(db_err)?;
        let fm: serde_json::Value = serde_json::from_str(&fm).unwrap_or(serde_json::Value::Null);
        let status = fm.get("status").and_then(|v| v.as_str()).unwrap_or("claim");

        let supports_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM links l
             JOIN pages p ON p.slug = l.target_slug
             WHERE l.source_slug = ?
               AND l.edge_type = 'supports'
               AND p.page_type = 'artifact'",
        )
        .bind(&slug)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;

        if supports_count == 0 {
            if status == "draft" {
                warn_drafts.push(slug);
            } else {
                fail_claims.push(slug);
            }
        }
    }

    if !fail_claims.is_empty() {
        let actions: Vec<SuggestedAction> = fail_claims
            .iter()
            .map(|s| SuggestedAction::LinkEvidence {
                from: s.clone(),
                to: String::new(),
                link_type: "supports".into(),
            })
            .collect();
        return Ok(ValidatorResult::fail(
            "finding_has_supporting_artifact",
            format!("{} finding(s) without supports edge", fail_claims.len()),
        )
        .with_affected(fail_claims.clone())
        .with_actions(actions));
    }
    if !warn_drafts.is_empty() {
        return Ok(ValidatorResult::warn(
            "finding_has_supporting_artifact",
            format!(
                "{} draft finding(s) without supports edge (warn-only)",
                warn_drafts.len()
            ),
        )
        .with_affected(warn_drafts.clone()));
    }
    Ok(ValidatorResult::pass("finding_has_supporting_artifact"))
}

pub async fn finding_has_dataset_lineage(
    pool: &SqlitePool,
    run_slug: &str,
) -> Result<ValidatorResult> {
    // Only findings this run produced directly (run --produces--> finding).
    let rows = sqlx::query(
        "SELECT DISTINCT p.slug, p.frontmatter FROM pages p
         JOIN links l ON l.target_slug = p.slug
         WHERE p.page_type = 'finding'
           AND l.source_slug = ?1
           AND l.edge_type = 'produces'",
    )
    .bind(run_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    if rows.is_empty() {
        return Ok(ValidatorResult::warn(
            "finding_has_dataset_lineage",
            "no finding linked to research_run yet",
        ));
    }

    let mut warn_drafts: Vec<String> = Vec::new();
    let mut fail_claims: Vec<String> = Vec::new();

    for row in &rows {
        let slug: String = row.try_get("slug").map_err(db_err)?;
        let fm: String = row.try_get("frontmatter").map_err(db_err)?;
        let fm: serde_json::Value = serde_json::from_str(&fm).unwrap_or(serde_json::Value::Null);
        let status = fm.get("status").and_then(|v| v.as_str()).unwrap_or("claim");

        // Phase 3 graph contract: `uses_dataset` is run→dataset (dependency
        // declaration), `derived_from` is artifact→dataset (blood lineage).
        // The derived dataset must also be registered on this run; otherwise a
        // finding can pass using an artifact from a different research context.
        let lineage_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM links l1
             JOIN pages artifact ON artifact.slug = l1.target_slug
             JOIN links l2 ON l2.source_slug = artifact.slug
             JOIN pages dataset ON dataset.slug = l2.target_slug
             WHERE l1.source_slug = ?
               AND l1.edge_type = 'supports'
               AND artifact.page_type = 'artifact'
               AND l2.edge_type = 'derived_from'
               AND dataset.page_type = 'dataset'
               AND EXISTS (
                   SELECT 1 FROM links run_ds
                   WHERE run_ds.source_slug = ?
                     AND run_ds.target_slug = dataset.slug
                     AND run_ds.edge_type = 'uses_dataset'
               )",
        )
        .bind(&slug)
        .bind(run_slug)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;

        if lineage_count == 0 {
            if status == "draft" {
                warn_drafts.push(slug);
            } else {
                fail_claims.push(slug);
            }
        }
    }

    if !fail_claims.is_empty() {
        let actions: Vec<SuggestedAction> = fail_claims
            .iter()
            .map(|s| SuggestedAction::LinkEvidence {
                from: s.clone(),
                to: String::new(),
                link_type: "supports".into(),
            })
            .collect();
        return Ok(ValidatorResult::fail(
            "finding_has_dataset_lineage",
            format!(
                "{} finding(s) lack artifact → dataset provenance",
                fail_claims.len()
            ),
        )
        .with_affected(fail_claims.clone())
        .with_actions(actions));
    }
    if !warn_drafts.is_empty() {
        return Ok(ValidatorResult::warn(
            "finding_has_dataset_lineage",
            format!(
                "{} draft finding(s) lack artifact → dataset provenance",
                warn_drafts.len()
            ),
        )
        .with_affected(warn_drafts.clone()));
    }
    Ok(ValidatorResult::pass("finding_has_dataset_lineage"))
}

// ─── Literature-review validators (M3 Slice 1) ────────────────────────────────
//
// These scan globally because the literature_review pipeline does not yet
// link its outputs (synthesis / wiki) back to the originating research_run
// via `produces` edges. The `_run_slug` parameter is kept for API symmetry
// with the data-analysis validators and will be honored once Slice 3 wires
// the pipeline to emit `produces` edges.

const LITERATURE_MIN_SOURCE_COUNT: i64 = 3;
const SYNTHESIS_VALIDATOR_MIN_CITATIONS: usize = 2;

/// Source corpus must contain at least N pages of type `note` or `raw`,
/// otherwise downstream synthesis/review is unsupported.
pub async fn source_count_minimum(pool: &SqlitePool, _run_slug: &str) -> Result<ValidatorResult> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pages WHERE page_type IN ('note', 'raw')",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)?;

    Ok(if count >= LITERATURE_MIN_SOURCE_COUNT {
        ValidatorResult::pass("source_count_minimum")
    } else {
        ValidatorResult::fail(
            "source_count_minimum",
            format!(
                "only {count} source page(s); need ≥ {LITERATURE_MIN_SOURCE_COUNT} before literature review is meaningful"
            ),
        )
    })
}

/// Every `[[slug | chunk:N]]` reference in synthesis/wiki pages must point
/// to a `chunks.id` that actually exists in the database. Catches the
/// "LLM invented chunk:99999" failure mode.
pub async fn citation_chunks_exist(pool: &SqlitePool, _run_slug: &str) -> Result<ValidatorResult> {
    let refs = collect_chunk_refs(pool).await?;
    if refs.is_empty() {
        return Ok(ValidatorResult::warn(
            "citation_chunks_exist",
            "no chunk citations found on synthesis/wiki pages yet",
        ));
    }

    // Batch-lookup all referenced chunk IDs in one query.
    let ids: Vec<i64> = refs.iter().map(|r| r.chunk_id).collect();
    let existing = fetch_existing_chunk_ids(pool, &ids).await?;

    let missing: Vec<&ChunkRef> = refs.iter().filter(|r| !existing.contains(&r.chunk_id)).collect();
    if missing.is_empty() {
        return Ok(ValidatorResult::pass("citation_chunks_exist"));
    }

    let affected: Vec<String> = missing.iter().map(|r| r.host_slug.clone()).collect();
    let actions: Vec<SuggestedAction> = missing
        .iter()
        .map(|r| SuggestedAction::AddCitation {
            slug: r.host_slug.clone(),
            chunk_ref: Some(format!("{}|chunk:{}", r.target_slug, r.chunk_id)),
        })
        .collect();
    Ok(ValidatorResult::fail(
        "citation_chunks_exist",
        format!("{} citation(s) reference non-existent chunk ID(s)", missing.len()),
    )
    .with_affected(affected)
    .with_actions(actions))
}

/// Every `[[slug | chunk:N]]` reference must point to a chunk whose
/// `page_slug` matches the cited slug. Catches "cited chunk:5 to article A
/// but chunk 5 actually belongs to article B".
pub async fn citation_chunk_matches_slug(pool: &SqlitePool, _run_slug: &str) -> Result<ValidatorResult> {
    let refs = collect_chunk_refs(pool).await?;
    if refs.is_empty() {
        return Ok(ValidatorResult::warn(
            "citation_chunk_matches_slug",
            "no chunk citations found on synthesis/wiki pages yet",
        ));
    }

    let ids: Vec<i64> = refs.iter().map(|r| r.chunk_id).collect();
    let id_to_slug = fetch_chunk_id_to_slug(pool, &ids).await?;

    // Only count mismatches for chunks that exist; non-existence is the
    // job of `citation_chunks_exist` so we don't double-report.
    let mismatches: Vec<&ChunkRef> = refs
        .iter()
        .filter(|r| match id_to_slug.get(&r.chunk_id) {
            Some(actual) => actual != &r.target_slug,
            None => false,
        })
        .collect();

    if mismatches.is_empty() {
        return Ok(ValidatorResult::pass("citation_chunk_matches_slug"));
    }

    let affected: Vec<String> = mismatches.iter().map(|r| r.host_slug.clone()).collect();
    let actions: Vec<SuggestedAction> = mismatches
        .iter()
        .map(|r| SuggestedAction::AddCitation {
            slug: r.host_slug.clone(),
            chunk_ref: Some(format!(
                "cited {} but chunk {} belongs to {}",
                r.target_slug,
                r.chunk_id,
                id_to_slug.get(&r.chunk_id).cloned().unwrap_or_default()
            )),
        })
        .collect();
    Ok(ValidatorResult::fail(
        "citation_chunk_matches_slug",
        format!(
            "{} citation(s) cite a chunk whose actual page slug differs",
            mismatches.len()
        ),
    )
    .with_affected(affected)
    .with_actions(actions))
}

/// Every `synthesis` page must satisfy structural quality rules: enough
/// traceable citations, bounded section count, few uncited substantive
/// sections, few thin sections. The pure check is shared with the pipeline
/// retry gate via [`check_synthesis_quality`].
pub async fn synthesis_sections_have_citations(
    pool: &SqlitePool,
    _run_slug: &str,
) -> Result<ValidatorResult> {
    let rows = sqlx::query(
        "SELECT slug, compiled_truth FROM pages WHERE page_type = 'synthesis'",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    if rows.is_empty() {
        return Ok(ValidatorResult::warn(
            "synthesis_sections_have_citations",
            "no synthesis pages yet",
        ));
    }

    let mut failed: Vec<(String, String)> = Vec::new();
    for row in &rows {
        let slug: String = row.try_get("slug").map_err(db_err)?;
        let content: String = row.try_get("compiled_truth").map_err(db_err)?;
        if let Err(reason) =
            check_synthesis_quality(&content, SYNTHESIS_VALIDATOR_MIN_CITATIONS)
        {
            failed.push((slug, reason));
        }
    }

    if failed.is_empty() {
        return Ok(ValidatorResult::pass("synthesis_sections_have_citations"));
    }

    let message = format!(
        "{}/{} synthesis page(s) failed quality check: {}",
        failed.len(),
        rows.len(),
        failed
            .iter()
            .take(3)
            .map(|(s, r)| format!("{s} ({r})"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let affected: Vec<String> = failed.iter().map(|(s, _)| s.clone()).collect();
    Ok(ValidatorResult::fail("synthesis_sections_have_citations", message).with_affected(affected))
}

/// Every `wiki` page (the final composed review) must cite at least one
/// `synthesis` page; a review composed without grounding in synthesis
/// pages bypasses the literature-review pipeline's evidence layer.
pub async fn review_links_to_synthesis_pages(
    pool: &SqlitePool,
    _run_slug: &str,
) -> Result<ValidatorResult> {
    let wiki_slugs: Vec<String> = sqlx::query_scalar(
        "SELECT slug FROM pages WHERE page_type = 'wiki'",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    if wiki_slugs.is_empty() {
        return Ok(ValidatorResult::warn(
            "review_links_to_synthesis_pages",
            "no wiki/review page composed yet",
        ));
    }

    let mut unbacked: Vec<String> = Vec::new();
    for slug in &wiki_slugs {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM links l
             JOIN pages p ON p.slug = l.target_slug
             WHERE l.source_slug = ? AND p.page_type = 'synthesis'",
        )
        .bind(slug)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
        if count == 0 {
            unbacked.push(slug.clone());
        }
    }

    if unbacked.is_empty() {
        return Ok(ValidatorResult::pass("review_links_to_synthesis_pages"));
    }

    Ok(ValidatorResult::fail(
        "review_links_to_synthesis_pages",
        format!(
            "{} wiki page(s) do not cite any synthesis page",
            unbacked.len()
        ),
    )
    .with_affected(unbacked))
}

// ─── Synthesis quality core ───────────────────────────────────────────────────
//
// Shared between the pipeline retry gate (engine.rs) and
// `synthesis_sections_have_citations`. Keeping the rules in one place
// prevents the validator and the gate from drifting apart.

/// Check structural / citation rules of a synthesis page. `source_count`
/// caps the required citation count via `source_count.clamp(1, 3)`.
///
/// Pipeline-time callers pass the actual linked source count; validator
/// callers pass [`SYNTHESIS_VALIDATOR_MIN_CITATIONS`] because that
/// per-page context isn't recoverable from persisted state.
pub fn check_synthesis_quality(
    content: &str,
    source_count: usize,
) -> std::result::Result<(), String> {
    let citation_count = content.matches("| chunk:").count();
    let required_citations = source_count.clamp(1, 3);
    if citation_count < required_citations {
        return Err(format!(
            "only {citation_count} traceable citation(s), expected at least {required_citations}"
        ));
    }

    let sections = synthesis_sections(content);
    let section_count = sections.len();
    let max_sections = if source_count > 8 { 20 } else { 12 };
    if section_count > max_sections {
        return Err(format!(
            "{section_count} second-level section(s), maximum is {max_sections} for {source_count} source(s)"
        ));
    }

    let mut uncited_body_sections = 0usize;
    let mut thin_sections = 0usize;
    for (heading, body) in &sections {
        let exempt = is_synthesis_meta_section(heading);
        let has_citation = body.contains("| chunk:");
        let body_chars = body.trim().chars().count();
        if !exempt && !has_citation {
            uncited_body_sections += 1;
        }
        if !exempt && body_chars < 80 {
            thin_sections += 1;
        }
    }

    if uncited_body_sections > 2 {
        return Err(format!(
            "{uncited_body_sections} substantive section(s) have no traceable citation"
        ));
    }
    if thin_sections > 2 {
        return Err(format!("{thin_sections} substantive section(s) are too thin"));
    }

    Ok(())
}

fn synthesis_sections(content: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body = String::new();

    for line in content.lines() {
        if line.starts_with("## ") {
            if let Some(heading) = current_heading.replace(line.trim().to_string()) {
                sections.push((heading, current_body.trim().to_string()));
                current_body.clear();
            }
        } else if current_heading.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }

    if let Some(heading) = current_heading {
        sections.push((heading, current_body.trim().to_string()));
    }

    sections
}

fn is_synthesis_meta_section(heading: &str) -> bool {
    let heading = heading.trim_start_matches('#').trim();
    matches!(
        heading,
        "Working Judgment"
            | "Open Questions"
            | "综合判断"
            | "开放问题"
            | "待研究问题"
            | "未决问题"
    )
}

// ─── Chunk reference scanning helpers ─────────────────────────────────────────

#[derive(Debug, Clone)]
struct ChunkRef {
    /// The synthesis/wiki page that *contains* the `[[..|chunk:N]]` citation.
    host_slug: String,
    /// The slug cited inside `[[target_slug | chunk:N]]`.
    target_slug: String,
    /// The cited chunk ID.
    chunk_id: i64,
}

/// Scan all synthesis/wiki pages for `[[slug | chunk:N]]` references.
async fn collect_chunk_refs(pool: &SqlitePool) -> Result<Vec<ChunkRef>> {
    let rows = sqlx::query(
        "SELECT slug, compiled_truth FROM pages
         WHERE page_type IN ('synthesis', 'wiki')",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut refs = Vec::new();
    for row in &rows {
        let host_slug: String = row.try_get("slug").map_err(db_err)?;
        let content: String = row.try_get("compiled_truth").map_err(db_err)?;
        for link in extract_links(&content) {
            if let Some(chunk_id) = link.chunk_id {
                refs.push(ChunkRef {
                    host_slug: host_slug.clone(),
                    target_slug: link.target_slug,
                    chunk_id,
                });
            }
        }
    }
    Ok(refs)
}

async fn fetch_existing_chunk_ids(
    pool: &SqlitePool,
    ids: &[i64],
) -> Result<std::collections::HashSet<i64>> {
    let map = fetch_chunk_id_to_slug(pool, ids).await?;
    Ok(map.into_keys().collect())
}

async fn fetch_chunk_id_to_slug(
    pool: &SqlitePool,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, String>> {
    use std::collections::HashMap;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    // SQLite IN-clause with bound parameters. Deduplicate to keep the
    // placeholder list small for queries with repeated refs.
    let mut unique: Vec<i64> = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();

    let placeholders = vec!["?"; unique.len()].join(",");
    let sql = format!("SELECT id, page_slug FROM chunks WHERE id IN ({placeholders})");
    let mut q = sqlx::query(&sql);
    for id in &unique {
        q = q.bind(id);
    }
    let rows = q.fetch_all(pool).await.map_err(db_err)?;

    let mut out = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: i64 = row.try_get("id").map_err(db_err)?;
        let slug: String = row.try_get("page_slug").map_err(db_err)?;
        out.insert(id, slug);
    }
    Ok(out)
}
