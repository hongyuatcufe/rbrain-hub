//! Minimum M1 validators. Each inspects DB state for a given research_run
//! without executing analysis code (CLAUDE.md §5).
//!
//! Three validators are wired:
//!
//! - `dataset_registered`           — run has at least one `uses_dataset` link
//!   to a page with `page_type = 'dataset'`.
//! - `artifact_hash_present`        — every artifact-typed page linked from the
//!   run has a non-empty `hash` in its frontmatter.
//! - `finding_has_supporting_artifact` — every finding linked to the run has
//!   at least one `supports` outlink to an artifact page. Findings with
//!   `status = 'draft'` only raise a `warn`.

use rbrain_core::error::{BrainError, Result};
use sqlx::{Row, SqlitePool};

use super::actions::SuggestedAction;
use super::result::ValidatorResult;

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
