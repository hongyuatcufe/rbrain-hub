//! `brain_citation_check` thin wrapper around the existing `audit_citations`
//! implementation in `engine.rs`. Per CLAUDE.md §7, we do **not** rewrite
//! citation logic — we adapt the existing report shape into the
//! ValidatorResult vocabulary.

use rbrain_core::error::Result;
use serde::{Deserialize, Serialize};

use super::actions::SuggestedAction;
use super::result::{ValidatorResult, ValidatorStatus};
use crate::engine::{AuditFinding, AuditReport, Engine};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationCheckReport {
    pub slug: String,
    pub validator: ValidatorResult,
    pub raw_findings: Vec<CitationFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationFinding {
    pub severity: String,
    pub category: String,
    pub message: String,
    pub suggestion: Option<String>,
}

pub async fn run_citation_check(engine: &Engine, slug: &str) -> Result<CitationCheckReport> {
    let report = engine.audit_citations(slug, false).await?;
    Ok(adapt_report(slug, &report))
}

fn adapt_report(slug: &str, report: &AuditReport) -> CitationCheckReport {
    let raw_findings: Vec<CitationFinding> = report
        .findings
        .iter()
        .map(|f: &AuditFinding| CitationFinding {
            severity: f.severity.to_string(),
            category: f.category.to_string(),
            message: f.message.clone(),
            suggestion: f.suggestion.clone(),
        })
        .collect();

    let has_error = raw_findings.iter().any(|f| f.severity == "ERROR");
    let has_warn = raw_findings.iter().any(|f| f.severity == "WARN");

    let status = if has_error {
        ValidatorStatus::Fail
    } else if has_warn {
        ValidatorStatus::Warn
    } else {
        ValidatorStatus::Pass
    };

    let message = if raw_findings.is_empty() {
        "all citations resolve to source pages".to_string()
    } else {
        format!(
            "{} error(s), {} warn(s) from rbrain audit",
            raw_findings
                .iter()
                .filter(|f| f.severity == "ERROR")
                .count(),
            raw_findings.iter().filter(|f| f.severity == "WARN").count(),
        )
    };

    let suggested_actions: Vec<SuggestedAction> = raw_findings
        .iter()
        .filter_map(|f| action_for(slug, f))
        .collect();

    CitationCheckReport {
        slug: slug.to_string(),
        validator: ValidatorResult {
            validator: "brain_citation_check".to_string(),
            status,
            message,
            affected_slugs: vec![slug.to_string()],
            suggested_actions,
        },
        raw_findings,
    }
}

fn action_for(slug: &str, f: &CitationFinding) -> Option<SuggestedAction> {
    match f.category.as_str() {
        // "non-source citation" → recommend re-citing a primary source
        "citation_type" => Some(SuggestedAction::AddCitation {
            slug: slug.to_string(),
            chunk_ref: None,
        }),
        // missing bib entry / dangling chunk → add a citation
        "bib_missing" | "bib_orphan" | "bib_duplicate" => Some(SuggestedAction::AddCitation {
            slug: slug.to_string(),
            chunk_ref: None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AuditFinding, AuditReport};

    fn rpt(findings: Vec<AuditFinding>) -> AuditReport {
        AuditReport {
            slug: "s".into(),
            findings,
            fixed: vec![],
        }
    }

    fn fnd(sev: &'static str, cat: &'static str) -> AuditFinding {
        AuditFinding {
            severity: sev,
            category: cat,
            message: "m".into(),
            suggestion: None,
            auto_fixable: false,
        }
    }

    #[test]
    fn empty_audit_yields_pass() {
        let r = adapt_report("s", &rpt(vec![]));
        assert_eq!(r.validator.status, ValidatorStatus::Pass);
        assert!(r.validator.suggested_actions.is_empty());
    }

    #[test]
    fn error_finding_becomes_fail_with_action() {
        let r = adapt_report("s", &rpt(vec![fnd("ERROR", "citation_type")]));
        assert_eq!(r.validator.status, ValidatorStatus::Fail);
        assert_eq!(r.validator.suggested_actions.len(), 1);
    }

    #[test]
    fn warn_only_yields_warn() {
        let r = adapt_report("s", &rpt(vec![fnd("WARN", "bib_orphan")]));
        assert_eq!(r.validator.status, ValidatorStatus::Warn);
    }
}
