-- Migration 0013: research_runs — durable identity for ZeroClaw-driven research runs.
--
-- The Markdown page (page_type = 'research_run') is the rendering layer; this
-- table is the source of truth for run identity, lifecycle status, and the
-- fingerprint used by validators / protocol state machine.
--
-- See: rbrain-hub-execution-plan.md (Phase 1, Phase 8) and CLAUDE.md.

CREATE TABLE IF NOT EXISTS research_runs (
    id TEXT PRIMARY KEY,                -- uuid (caller-provided)
    slug TEXT NOT NULL UNIQUE
        REFERENCES pages(slug) ON DELETE CASCADE,
    task_type TEXT NOT NULL CHECK(task_type IN (
        'literature_review','data_analysis','mixed_methods','theory_building'
    )),
    status TEXT NOT NULL CHECK(status IN (
        'planned','running','validating','complete','blocked'
    )),
    fingerprint TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT 'zeroclaw',
    started_at TEXT,
    completed_at TEXT,
    last_validated_at TEXT,
    last_validation_summary TEXT,       -- JSON
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_research_runs_status
    ON research_runs(status);

CREATE INDEX IF NOT EXISTS idx_research_runs_task_type
    ON research_runs(task_type);
