//! Controlled-vocabulary suggested actions (CLAUDE.md §5).
//!
//! Each variant carries its own typed payload so ZeroClaw can dispatch
//! `auto-fix where safe` without parsing free-form strings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SuggestedAction {
    RegisterDataset {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
    },
    RegisterArtifact {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
    },
    RecordAnalysisPlan {
        run_slug: String,
    },
    LinkEvidence {
        from: String,
        to: String,
        link_type: String,
    },
    RecordLimitation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finding_slug: Option<String>,
    },
    RerunAnalysis {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    AddCitation {
        slug: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chunk_ref: Option<String>,
    },
    SplitFinding {
        finding_slug: String,
    },
    AddCodebook {
        dataset_slug: String,
    },
    HashMismatchReupload {
        slug: String,
        expected_hash: String,
        actual_hash: String,
    },
}
