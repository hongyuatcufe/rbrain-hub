//! Provenance edge vocabulary (Phase 3).
//!
//! These are the **research-context** edges. Generic graph edges that
//! existed before M2 (`references`, `mentions`, `related`, `supports`,
//! `contrasts`, etc.) keep working through `engine.add_link` — this module
//! only catalogs the subset that validators and evidence/provenance walks
//! understand semantically.

use std::fmt;

/// An edge in the research provenance graph. See `rbrain-hub-execution-plan.md`
/// Phase 3 for the graph patterns each edge participates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResearchEdge {
    /// run → dataset. "this run depends on this dataset"
    UsesDataset,
    /// run → variable (codebook entry). "this run reads this variable"
    UsesVariable,
    /// run → method_note. "this run applies this method"
    UsesMethod,
    /// result → script. "this result was computed by this script"
    ComputedBy,
    /// artifact|result → dataset. "this artifact's content traces back to that dataset"
    DerivedFrom,
    /// finding → artifact|literature_chunk. "this evidence supports this finding"
    Supports,
    /// finding → finding|literature. "this finding contradicts that one"
    Contradicts,
    /// analysis_plan → research_question. "this plan tests this hypothesis"
    TestsHypothesis,
    /// page → source. "this page cites that source"
    Cites,
    /// limitation → finding. "this limitation limits that finding"
    Limits,
    /// run → finding|artifact. "this run produced that output"
    Produces,
    /// validator|reviewer → claim. "this validates that"
    Validates,
}

impl ResearchEdge {
    /// Canonical string form used in the SQLite `links.edge_type` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            ResearchEdge::UsesDataset => "uses_dataset",
            ResearchEdge::UsesVariable => "uses_variable",
            ResearchEdge::UsesMethod => "uses_method",
            ResearchEdge::ComputedBy => "computed_by",
            ResearchEdge::DerivedFrom => "derived_from",
            ResearchEdge::Supports => "supports",
            ResearchEdge::Contradicts => "contradicts",
            ResearchEdge::TestsHypothesis => "tests_hypothesis",
            ResearchEdge::Cites => "cites",
            ResearchEdge::Limits => "limits",
            ResearchEdge::Produces => "produces",
            ResearchEdge::Validates => "validates",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        ALL.iter().copied().find(|e| e.as_str() == s)
    }
}

impl fmt::Display for ResearchEdge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// All research-context edges. Exposed so the doctor and lints can iterate.
pub const ALL: &[ResearchEdge] = &[
    ResearchEdge::UsesDataset,
    ResearchEdge::UsesVariable,
    ResearchEdge::UsesMethod,
    ResearchEdge::ComputedBy,
    ResearchEdge::DerivedFrom,
    ResearchEdge::Supports,
    ResearchEdge::Contradicts,
    ResearchEdge::TestsHypothesis,
    ResearchEdge::Cites,
    ResearchEdge::Limits,
    ResearchEdge::Produces,
    ResearchEdge::Validates,
];

/// True iff `s` is a known research edge. Used by `is_research_edge_strict`
/// to warn callers building research graphs about typos.
pub fn is_research_edge(s: &str) -> bool {
    ResearchEdge::try_from_str(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_edges() {
        for e in ALL {
            assert_eq!(ResearchEdge::try_from_str(e.as_str()), Some(*e));
        }
    }

    #[test]
    fn unknown_edge_is_not_research() {
        assert!(!is_research_edge("references"));
        assert!(!is_research_edge("uses_datasets")); // typo
        assert!(is_research_edge("uses_dataset"));
    }
}
