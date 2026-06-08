//! Protocol state derivation. Given a research_run + the most recent validator
//! report, returns the **next** action ZeroClaw should take.
//!
//! No state lives in frontmatter; everything is derived from
//! `research_runs.last_validation_summary` and the active task_type.

use serde::{Deserialize, Serialize};

use super::model::{ResearchRun, RunStatus, TaskType};
use crate::evidence::{SuggestedAction, ValidatorResult, ValidatorStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextAction {
    pub action: SuggestedAction,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolState {
    pub run_id: String,
    pub task_type: TaskType,
    pub current_step: String,
    pub completed_steps: Vec<String>,
    pub next_actions: Vec<NextAction>,
    pub blocking_validators: Vec<ValidatorResult>,
}

/// Compute the protocol state from a run + (optional) latest validator results.
///
/// Logic:
///   - `Planned`            → step `create_research_run` is the most recent
///                            completed step; next is `register_dataset` or
///                            `record_analysis_plan` depending on task_type.
///   - `Running/Validating` → walk task-type checklist; first failing/missing
///                            validator names the current step.
///   - `Complete`           → current step `done`, no next actions.
///   - `Blocked`            → echo the blocking validators verbatim.
pub fn derive_state(run: &ResearchRun, validators: &[ValidatorResult]) -> ProtocolState {
    let checklist = checklist_for(run.task_type);

    let blocking: Vec<ValidatorResult> = validators
        .iter()
        .filter(|v| v.status == ValidatorStatus::Fail)
        .cloned()
        .collect();

    let (completed_steps, current_step) = match run.status {
        RunStatus::Planned => (
            vec!["create_research_run".to_string()],
            checklist
                .get(1)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "done".to_string()),
        ),
        RunStatus::Complete => (
            checklist.iter().map(|s| s.to_string()).collect(),
            "done".to_string(),
        ),
        _ => {
            let mut completed = Vec::new();
            let mut current = checklist
                .last()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "done".to_string());
            for step in checklist.iter() {
                if step_is_satisfied(step, validators) {
                    completed.push(step.to_string());
                } else {
                    current = step.to_string();
                    break;
                }
            }
            (completed, current)
        }
    };

    let next_actions = if matches!(run.status, RunStatus::Complete) {
        Vec::new()
    } else {
        validators
            .iter()
            .filter(|v| v.status != ValidatorStatus::Pass)
            .flat_map(|v| {
                v.suggested_actions.iter().map(|a| NextAction {
                    action: a.clone(),
                    reason: v.validator.clone(),
                })
            })
            .collect()
    };

    ProtocolState {
        run_id: run.id.clone(),
        task_type: run.task_type,
        current_step,
        completed_steps,
        next_actions,
        blocking_validators: blocking,
    }
}

fn checklist_for(task_type: TaskType) -> &'static [&'static str] {
    match task_type {
        TaskType::DataAnalysis => &[
            "create_research_run",
            "register_dataset",
            "record_analysis_plan",
            "handoff_to_zeroclaw_for_execution",
            "register_analysis_artifacts",
            "validate_result_provenance",
            "validate_findings",
            "compose_research_memo",
        ],
        TaskType::LiteratureReview => &[
            "create_research_run",
            "import_sources",
            "sync_embed",
            "extract_concepts",
            "synthesize_by_concept",
            "compose_review_draft",
            "validate_citations",
            "detect_gaps",
            "detect_contradictions",
            "save_research_memo",
        ],
        TaskType::MixedMethods | TaskType::TheoryBuilding => &[
            "create_research_run",
            "record_analysis_plan",
            "handoff_to_zeroclaw_for_execution",
            "register_analysis_artifacts",
            "validate_findings",
            "compose_research_memo",
        ],
    }
}

/// A step is satisfied iff:
///   - it is implicit/manual (no validator wired), OR
///   - its mapped validator is present and `pass`.
///
/// "Validator missing entirely" (i.e. the validator name is known but no
/// result was supplied yet) returns false → the walk stops at this step.
///
/// M1 wires only `dataset_registered`, `artifact_hash_present`, and
/// `finding_has_supporting_artifact`. The other validator names are placeholder
/// stops that M2/M3 will fill in.
fn step_is_satisfied(step: &str, validators: &[ValidatorResult]) -> bool {
    match validator_for_step(step) {
        StepKind::Manual => true,
        StepKind::Validator(expected) => validators
            .iter()
            .any(|v| v.validator == expected && v.status == ValidatorStatus::Pass),
    }
}

enum StepKind {
    Manual,
    Validator(&'static str),
}

fn validator_for_step(step: &str) -> StepKind {
    match step {
        // Implicit / orchestration-only steps:
        "create_research_run"
        | "handoff_to_zeroclaw_for_execution"
        | "compose_research_memo"
        | "import_sources"
        | "sync_embed"
        | "extract_concepts"
        | "synthesize_by_concept"
        | "compose_review_draft"
        | "save_research_memo" => StepKind::Manual,

        // Validator-backed steps:
        "register_dataset" => StepKind::Validator("dataset_registered"),
        "record_analysis_plan" => StepKind::Validator("analysis_plan_exists"),
        "register_analysis_artifacts" => StepKind::Validator("artifact_hash_present"),
        "validate_result_provenance" => StepKind::Validator("finding_has_dataset_lineage"),
        "validate_findings" => StepKind::Validator("finding_has_supporting_artifact"),
        "validate_citations" => StepKind::Validator("citation_chunks_exist"),
        "detect_gaps" => StepKind::Validator("gap_analysis_present"),
        "detect_contradictions" => StepKind::Validator("contradictions_recorded"),
        _ => StepKind::Manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{SuggestedAction, ValidatorResult, ValidatorStatus};

    fn run(status: RunStatus, task_type: TaskType) -> ResearchRun {
        ResearchRun {
            id: "r1".into(),
            slug: "research/runs/r1".into(),
            task_type,
            status,
            fingerprint: String::new(),
            created_by: "zeroclaw".into(),
            started_at: None,
            completed_at: None,
            last_validated_at: None,
            last_validation_summary: None,
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    fn vr(name: &str, status: ValidatorStatus) -> ValidatorResult {
        ValidatorResult {
            validator: name.into(),
            status,
            message: String::new(),
            affected_slugs: vec![],
            suggested_actions: vec![],
        }
    }

    #[test]
    fn planned_run_points_to_first_step_after_create() {
        let state = derive_state(&run(RunStatus::Planned, TaskType::DataAnalysis), &[]);
        assert_eq!(state.completed_steps, vec!["create_research_run"]);
        assert_eq!(state.current_step, "register_dataset");
    }

    #[test]
    fn running_run_advances_when_validator_passes() {
        let validators = vec![vr("dataset_registered", ValidatorStatus::Pass)];
        let state = derive_state(
            &run(RunStatus::Running, TaskType::DataAnalysis),
            &validators,
        );
        assert!(state.completed_steps.contains(&"register_dataset".into()));
        assert_eq!(state.current_step, "record_analysis_plan");
    }

    #[test]
    fn blocking_validators_surface() {
        let mut v = vr("dataset_registered", ValidatorStatus::Fail);
        v.suggested_actions = vec![SuggestedAction::RegisterDataset {
            hint: Some("outputs/data.csv".into()),
        }];
        let state = derive_state(
            &run(RunStatus::Running, TaskType::DataAnalysis),
            &[v.clone()],
        );
        assert_eq!(state.blocking_validators.len(), 1);
        assert_eq!(state.next_actions.len(), 1);
    }
}
