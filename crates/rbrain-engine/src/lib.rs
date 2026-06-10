pub mod citation;
pub mod engine;
pub mod evidence;
pub mod links;
pub mod pipeline;
pub mod research;
pub mod token_budget;

pub use engine::{
    BrainStats, ChunkResult, CitationHint, CitationSummary, CitationVerification,
    CiteEntry, DocCitationReport, Engine, ExplainedHit, GraphEdge, MergeRecord, VerifyStatus,
};
pub use evidence::{
    CitationCheckReport, SuggestedAction, ValidatorResult, ValidatorStatus, analysis_plan_exists,
    artifact_hash_present, dataset_registered, finding_has_dataset_lineage,
    finding_has_supporting_artifact, run_citation_check,
};
pub use links::{LinkRef, extract_links};
pub use pipeline::{
    InputSpec, OutputMode, PageBatch, PipelineProfile, PipelineStep, PromptSpec, ResponseFormat,
    RetryParser, Runnable, SaveTypeConfig, Sequential, StageConfig,
};
pub use rbrain_search::TantivyIndex;
pub use research::{
    ADMIN_USER, DEFAULT_PROJECT, DEFAULT_USER, GLOBAL_USER, NextAction, Project, ProjectStatus,
    ProjectStore, ProtocolState, ResearchEdge, ResearchRun, ResearchRunStore, RunStatus, TaskType,
    TenantContext, is_research_edge,
};
