//! Research run state machine and protocol-state derivation.
//!
//! See `rbrain-hub-execution-plan.md` (Phase 1, Phase 2, Phase 8) and
//! `CLAUDE.md` for the architectural contract:
//!
//! - `research_runs` is the durable source of truth (migration 0013).
//! - Page (`page_type = research_run`) is the rendering layer.
//! - Protocol is **not** a static checklist; it is derived from validator
//!   results and exposed via [`ProtocolState`].
//! - ZeroClaw executes; rbrain records + validates.

pub mod model;
pub mod protocol;
pub mod store;

pub use model::{ResearchRun, RunStatus, TaskType};
pub use protocol::{NextAction, ProtocolState};
pub use store::ResearchRunStore;
