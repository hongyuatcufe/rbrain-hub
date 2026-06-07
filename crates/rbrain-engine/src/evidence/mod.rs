//! Evidence / citation / provenance validation primitives shared by
//! literature-review and data-analysis profiles.
//!
//! See `rbrain-hub-execution-plan.md` (Phase 2, Phase 4) and `CLAUDE.md`
//! sections 5–6 for the contract:
//!
//! - Validators **never execute analysis code**; they inspect registered state.
//! - Output uses [`ValidatorResult`] with `pass | warn | fail` and a
//!   controlled-enum [`SuggestedAction`] list for ZeroClaw to act on.

pub mod actions;
pub mod citation;
pub mod result;
pub mod validators;

pub use actions::SuggestedAction;
pub use citation::{CitationCheckReport, run_citation_check};
pub use result::{ValidatorResult, ValidatorStatus};
pub use validators::{
    analysis_plan_exists, artifact_hash_present, dataset_registered, finding_has_dataset_lineage,
    finding_has_supporting_artifact,
};
