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
pub mod evidence_walk;
pub mod provenance;
pub mod result;
pub mod validators;

pub use actions::SuggestedAction;
pub use citation::{CitationCheckReport, run_citation_check};
pub use evidence_walk::{
    EvidenceChain, EvidenceNode, EvidenceReport, run_evidence_check, run_evidence_check_with_ctx,
};
pub use provenance::{ProvenanceEdge, ProvenanceReport, provenance_of, provenance_of_with_ctx};
pub use result::{ValidatorResult, ValidatorStatus};
pub use validators::{
    analysis_plan_exists, artifact_hash_present, bibliography_consistency, check_synthesis_quality,
    citation_chunk_matches_slug, citation_chunks_exist, contradictions_recorded,
    dataset_registered, finding_has_dataset_lineage, finding_has_supporting_artifact,
    gap_analysis_present, primary_source_ratio, review_links_to_synthesis_pages,
    source_count_minimum, synthesis_sections_have_citations,
};
