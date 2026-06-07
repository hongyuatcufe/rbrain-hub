//! `brain_evidence_check` — walks the provenance graph from a finding and
//! reports whether it terminates in evidence the validators recognise.
//!
//! Two finding shapes are handled (driven by the supports edge target):
//!
//! - **data_analysis**:
//!     `finding --supports--> artifact (page_type='artifact')
//!              --derived_from--> dataset (page_type='dataset')`
//!     and optionally an intermediate `--computed_by--> script`.
//!
//! - **literature_review**:
//!     `finding --supports--> note|raw (page_type ∈ source-tier)`
//!     or `finding --cites--> note|raw` chunk reference.
//!
//! The walk uses two SQL queries (one per hop). Cheap, deterministic, no LLM.
//!
//! Returns [`EvidenceReport`] with the resolved chain so ZeroClaw can present
//! a human-readable provenance trail.

use rbrain_core::error::{BrainError, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use super::actions::SuggestedAction;
use super::result::{ValidatorResult, ValidatorStatus};

fn db_err<E: std::fmt::Display>(e: E) -> BrainError {
    BrainError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceChain {
    /// The finding under inspection.
    pub finding_slug: String,
    /// Pages reached via `supports` from the finding.
    pub direct_support: Vec<EvidenceNode>,
    /// Datasets reached via `supports → derived_from` 2-hop.
    pub datasets: Vec<String>,
    /// Scripts reached via `supports → computed_by` 2-hop.
    pub scripts: Vec<String>,
    /// Literature sources reached via `cites` 1-hop (literature finding shape).
    pub literature_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceNode {
    pub slug: String,
    pub page_type: String,
    pub via: String, // edge type used to reach this node
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceReport {
    pub chain: EvidenceChain,
    pub validator: ValidatorResult,
}

/// Source-tier page types — literature corpus.
fn is_source_tier(page_type: &str) -> bool {
    matches!(page_type, "note" | "raw")
}

pub async fn run_evidence_check(pool: &SqlitePool, finding_slug: &str) -> Result<EvidenceReport> {
    // ── 1. supports edges (data-analysis finding shape) ────────────────────
    let supports_rows = sqlx::query(
        "SELECT p.slug, p.page_type FROM links l
         JOIN pages p ON p.slug = l.target_slug
         WHERE l.source_slug = ? AND l.edge_type = 'supports'",
    )
    .bind(finding_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut direct_support: Vec<EvidenceNode> = Vec::new();
    for row in &supports_rows {
        direct_support.push(EvidenceNode {
            slug: row.try_get("slug").map_err(db_err)?,
            page_type: row.try_get("page_type").map_err(db_err)?,
            via: "supports".into(),
        });
    }

    // ── 2. cites edges (literature finding shape) ──────────────────────────
    let cites_rows = sqlx::query(
        "SELECT p.slug, p.page_type FROM links l
         JOIN pages p ON p.slug = l.target_slug
         WHERE l.source_slug = ? AND l.edge_type = 'cites'
               AND p.page_type IN ('note','raw')",
    )
    .bind(finding_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut literature_sources: Vec<String> = Vec::new();
    for row in &cites_rows {
        let slug: String = row.try_get("slug").map_err(db_err)?;
        literature_sources.push(slug.clone());
        direct_support.push(EvidenceNode {
            slug,
            page_type: row.try_get("page_type").map_err(db_err)?,
            via: "cites".into(),
        });
    }

    // Also count source-tier pages reached via supports (lit-review findings
    // often use `supports → note` directly without going through artifact).
    for node in &direct_support {
        if node.via == "supports" && is_source_tier(&node.page_type) {
            if !literature_sources.iter().any(|s| s == &node.slug) {
                literature_sources.push(node.slug.clone());
            }
        }
    }

    // ── 3. 2-hop: supports → derived_from → dataset ────────────────────────
    let dataset_rows = sqlx::query(
        "SELECT DISTINCT dataset.slug FROM links l1
         JOIN pages artifact ON artifact.slug = l1.target_slug
         JOIN links l2 ON l2.source_slug = artifact.slug
         JOIN pages dataset ON dataset.slug = l2.target_slug
         WHERE l1.source_slug = ?
           AND l1.edge_type = 'supports'
           AND artifact.page_type = 'artifact'
           AND l2.edge_type = 'derived_from'
           AND dataset.page_type = 'dataset'",
    )
    .bind(finding_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let datasets: Vec<String> = dataset_rows
        .iter()
        .map(|r| r.try_get("slug").map_err(db_err))
        .collect::<Result<Vec<_>>>()?;

    // ── 4. 2-hop: supports → computed_by → script ──────────────────────────
    let script_rows = sqlx::query(
        "SELECT DISTINCT script.slug FROM links l1
         JOIN pages artifact ON artifact.slug = l1.target_slug
         JOIN links l2 ON l2.source_slug = artifact.slug
         JOIN pages script ON script.slug = l2.target_slug
         WHERE l1.source_slug = ?
           AND l1.edge_type = 'supports'
           AND artifact.page_type = 'artifact'
           AND l2.edge_type = 'computed_by'
           AND script.page_type IN ('script','artifact')",
    )
    .bind(finding_slug)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let scripts: Vec<String> = script_rows
        .iter()
        .map(|r| r.try_get("slug").map_err(db_err))
        .collect::<Result<Vec<_>>>()?;

    let chain = EvidenceChain {
        finding_slug: finding_slug.to_string(),
        direct_support,
        datasets,
        scripts,
        literature_sources,
    };

    // ── 5. Status decision ─────────────────────────────────────────────────
    let validator = classify_chain(&chain);

    Ok(EvidenceReport { chain, validator })
}

fn classify_chain(chain: &EvidenceChain) -> ValidatorResult {
    if chain.direct_support.is_empty() {
        return ValidatorResult::fail(
            "brain_evidence_check",
            "finding has no `supports` or `cites` outlink",
        )
        .with_affected([chain.finding_slug.clone()])
        .with_actions([SuggestedAction::LinkEvidence {
            from: chain.finding_slug.clone(),
            to: String::new(),
            link_type: "supports".into(),
        }]);
    }

    let has_artifact = chain
        .direct_support
        .iter()
        .any(|n| n.via == "supports" && n.page_type == "artifact");
    let has_literature = !chain.literature_sources.is_empty();

    if has_artifact {
        // Data-analysis shape — require dataset lineage.
        if chain.datasets.is_empty() {
            return ValidatorResult::warn(
                "brain_evidence_check",
                "finding supports an artifact but no `derived_from → dataset` was reached within 2 hops",
            )
            .with_affected([chain.finding_slug.clone()]);
        }
        return ValidatorResult::pass("brain_evidence_check");
    }
    if has_literature {
        // Literature-review shape — accept supports/cites onto a source-tier page.
        return ValidatorResult::pass("brain_evidence_check");
    }
    // supports edges exist, but neither artifact-shaped nor literature-shaped.
    ValidatorResult::warn(
        "brain_evidence_check",
        "finding has support links but none resolve to an artifact or source-tier page",
    )
    .with_affected([chain.finding_slug.clone()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chain_fails() {
        let chain = EvidenceChain {
            finding_slug: "f1".into(),
            direct_support: vec![],
            datasets: vec![],
            scripts: vec![],
            literature_sources: vec![],
        };
        let v = classify_chain(&chain);
        assert_eq!(v.status, ValidatorStatus::Fail);
        assert_eq!(v.suggested_actions.len(), 1);
    }

    #[test]
    fn artifact_without_dataset_warns() {
        let chain = EvidenceChain {
            finding_slug: "f1".into(),
            direct_support: vec![EvidenceNode {
                slug: "art".into(),
                page_type: "artifact".into(),
                via: "supports".into(),
            }],
            datasets: vec![],
            scripts: vec![],
            literature_sources: vec![],
        };
        assert_eq!(classify_chain(&chain).status, ValidatorStatus::Warn);
    }

    #[test]
    fn artifact_with_dataset_passes() {
        let chain = EvidenceChain {
            finding_slug: "f1".into(),
            direct_support: vec![EvidenceNode {
                slug: "art".into(),
                page_type: "artifact".into(),
                via: "supports".into(),
            }],
            datasets: vec!["d".into()],
            scripts: vec![],
            literature_sources: vec![],
        };
        assert_eq!(classify_chain(&chain).status, ValidatorStatus::Pass);
    }

    #[test]
    fn literature_cite_passes() {
        let chain = EvidenceChain {
            finding_slug: "f1".into(),
            direct_support: vec![EvidenceNode {
                slug: "raw/articles/x".into(),
                page_type: "raw".into(),
                via: "cites".into(),
            }],
            datasets: vec![],
            scripts: vec![],
            literature_sources: vec!["raw/articles/x".into()],
        };
        assert_eq!(classify_chain(&chain).status, ValidatorStatus::Pass);
    }
}
