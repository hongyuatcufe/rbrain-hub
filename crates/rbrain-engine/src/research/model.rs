//! Research-run domain types. Mirrors the `research_runs` table (migration 0013).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    LiteratureReview,
    DataAnalysis,
    MixedMethods,
    TheoryBuilding,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::LiteratureReview => "literature_review",
            TaskType::DataAnalysis => "data_analysis",
            TaskType::MixedMethods => "mixed_methods",
            TaskType::TheoryBuilding => "theory_building",
        }
    }
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "literature_review" => Ok(TaskType::LiteratureReview),
            "data_analysis" => Ok(TaskType::DataAnalysis),
            "mixed_methods" => Ok(TaskType::MixedMethods),
            "theory_building" => Ok(TaskType::TheoryBuilding),
            other => Err(format!("unknown task_type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Planned,
    Running,
    Validating,
    Complete,
    Blocked,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Planned => "planned",
            RunStatus::Running => "running",
            RunStatus::Validating => "validating",
            RunStatus::Complete => "complete",
            RunStatus::Blocked => "blocked",
        }
    }

    /// Permitted forward transitions. Backwards transitions are allowed only
    /// from `validating | blocked → running` to support retries.
    pub fn can_transition_to(self, next: RunStatus) -> bool {
        use RunStatus::*;
        matches!(
            (self, next),
            (Planned, Running)
                | (Running, Validating)
                | (Running, Blocked)
                | (Validating, Running)
                | (Validating, Complete)
                | (Validating, Blocked)
                | (Blocked, Running)
        )
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "planned" => Ok(RunStatus::Planned),
            "running" => Ok(RunStatus::Running),
            "validating" => Ok(RunStatus::Validating),
            "complete" => Ok(RunStatus::Complete),
            "blocked" => Ok(RunStatus::Blocked),
            other => Err(format!("unknown run status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchRun {
    pub id: String,
    pub slug: String,
    pub task_type: TaskType,
    pub status: RunStatus,
    pub fingerprint: String,
    pub created_by: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_validated_at: Option<String>,
    pub last_validation_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_only_along_permitted_edges() {
        assert!(RunStatus::Planned.can_transition_to(RunStatus::Running));
        assert!(RunStatus::Running.can_transition_to(RunStatus::Validating));
        assert!(RunStatus::Validating.can_transition_to(RunStatus::Complete));
        assert!(RunStatus::Validating.can_transition_to(RunStatus::Running));
        assert!(RunStatus::Blocked.can_transition_to(RunStatus::Running));

        assert!(!RunStatus::Planned.can_transition_to(RunStatus::Complete));
        assert!(!RunStatus::Complete.can_transition_to(RunStatus::Running));
        assert!(!RunStatus::Running.can_transition_to(RunStatus::Planned));
    }

    #[test]
    fn task_type_round_trips() {
        for tt in [
            TaskType::LiteratureReview,
            TaskType::DataAnalysis,
            TaskType::MixedMethods,
            TaskType::TheoryBuilding,
        ] {
            assert_eq!(TaskType::from_str(tt.as_str()).unwrap(), tt);
        }
    }
}
