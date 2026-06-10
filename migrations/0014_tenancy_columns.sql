-- Migration 0014: multi-tenant columns on every user-content table.
--
-- Adds (user_id, project_id) to the data-bearing tables so a single SQLite
-- database can host many users and many projects per user. The columns
-- default to 'default' for backwards compatibility — existing single-tenant
-- brains keep working with no migration steps on the data plane. New code
-- paths always pass an explicit TenantContext.
--
-- Reserved user_id values (plan.md §M4 / decision #6):
--   'global' — pipeline-written content (e.g. ingested journal articles);
--              readable by every tenant whose project subscribes to its topic.
--   'admin'  — ZeroClaw / platform operator content; readable by admin
--              context only, surfaced to users via push_inbox.
--   'default' — backwards-compatible value for pre-M4 data and tests.

-- ── pages ──────────────────────────────────────────────────────────────────
ALTER TABLE pages ADD COLUMN user_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE pages ADD COLUMN project_id TEXT;
DROP INDEX IF EXISTS idx_pages_type;
DROP INDEX IF EXISTS idx_pages_updated;
DROP INDEX IF EXISTS idx_pages_lang;
CREATE INDEX idx_pages_user_type    ON pages(user_id, page_type);
CREATE INDEX idx_pages_user_updated ON pages(user_id, updated_at);
CREATE INDEX idx_pages_user_lang    ON pages(user_id, language);
CREATE INDEX idx_pages_user_project ON pages(user_id, project_id);

-- ── chunks ─────────────────────────────────────────────────────────────────
ALTER TABLE chunks ADD COLUMN user_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE chunks ADD COLUMN project_id TEXT;
DROP INDEX IF EXISTS idx_chunks_page;
CREATE INDEX idx_chunks_user_page         ON chunks(user_id, page_slug);
-- Re-create the existing partial indexes (which only mention status flags
-- and don't need the tenant prefix to stay correct):
-- idx_chunks_needs_embed and idx_chunks_needs_vec_index were already
-- dropped + recreated by migration 0012; leave them alone.

-- ── links ──────────────────────────────────────────────────────────────────
ALTER TABLE links ADD COLUMN user_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE links ADD COLUMN project_id TEXT;
DROP INDEX IF EXISTS idx_links_source;
DROP INDEX IF EXISTS idx_links_target;
DROP INDEX IF EXISTS idx_links_type;
CREATE INDEX idx_links_user_source ON links(user_id, source_slug);
CREATE INDEX idx_links_user_target ON links(user_id, target_slug);
CREATE INDEX idx_links_user_type   ON links(user_id, edge_type);

-- ── page_stats ────────────────────────────────────────────────────────────
ALTER TABLE page_stats ADD COLUMN user_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE page_stats ADD COLUMN project_id TEXT;
-- page_stats.slug is still PK; tenancy is just for filtering.
CREATE INDEX idx_page_stats_user ON page_stats(user_id);

-- ── jobs ───────────────────────────────────────────────────────────────────
ALTER TABLE jobs ADD COLUMN user_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE jobs ADD COLUMN project_id TEXT;
-- Don't touch the existing partial indexes on (queue, priority, status) —
-- the dispatcher selects by queue/status, not tenant. Add a per-tenant
-- secondary index for "list my jobs" UX.
CREATE INDEX idx_jobs_user ON jobs(user_id, created_at DESC);
