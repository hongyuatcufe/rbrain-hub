-- Migration 0015: projects — first-class container for a research project.
--
-- A project belongs to a user (owner_user_id). One project hosts many
-- research_runs (lit_review, data_analysis, etc.). See plan.md §M4.

CREATE TABLE projects (
    id              TEXT PRIMARY KEY,                  -- uuid
    slug            TEXT NOT NULL,                     -- short identifier, unique per owner
    owner_user_id   TEXT NOT NULL,
    title           TEXT NOT NULL,
    description     TEXT,
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK(status IN ('active','archived','complete')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(owner_user_id, slug)
) STRICT;

CREATE INDEX idx_projects_owner       ON projects(owner_user_id, status);
CREATE INDEX idx_projects_updated     ON projects(updated_at DESC);
