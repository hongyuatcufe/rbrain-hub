-- Migration 0016: make page_stats maintenance tenant-aware.
--
-- pages.slug is still globally unique in M4 PR-1, so page_stats keeps slug as
-- its primary key. The row inherits the target page's owner tenant, and indegree
-- counts only links written by that same tenant. User-created links to global
-- pages therefore do not inflate global relevance for everyone else.

DROP TRIGGER IF EXISTS update_indegree_insert;
DROP TRIGGER IF EXISTS update_indegree_delete;

CREATE TRIGGER update_indegree_insert AFTER INSERT ON links
BEGIN
    INSERT OR IGNORE INTO page_stats (slug, indegree, user_id, project_id)
    SELECT p.slug, 0, p.user_id, p.project_id
    FROM pages p
    WHERE p.slug = NEW.target_slug;

    UPDATE page_stats
    SET indegree = (
        SELECT COUNT(*)
        FROM links l
        WHERE l.target_slug = NEW.target_slug
          AND l.user_id = page_stats.user_id
          AND (
              l.project_id = page_stats.project_id
              OR (l.project_id IS NULL AND page_stats.project_id IS NULL)
          )
    )
    WHERE slug = NEW.target_slug;
END;

CREATE TRIGGER update_indegree_delete AFTER DELETE ON links
BEGIN
    UPDATE page_stats
    SET indegree = (
        SELECT COUNT(*)
        FROM links l
        WHERE l.target_slug = OLD.target_slug
          AND l.user_id = page_stats.user_id
          AND (
              l.project_id = page_stats.project_id
              OR (l.project_id IS NULL AND page_stats.project_id IS NULL)
          )
    )
    WHERE slug = OLD.target_slug;
END;

DELETE FROM page_stats;

INSERT OR IGNORE INTO page_stats (slug, indegree, user_id, project_id)
SELECT p.slug,
       COUNT(l.id) AS indegree,
       p.user_id,
       p.project_id
FROM pages p
LEFT JOIN links l
  ON l.target_slug = p.slug
 AND l.user_id = p.user_id
 AND (
     l.project_id = p.project_id
     OR (l.project_id IS NULL AND p.project_id IS NULL)
 )
GROUP BY p.slug, p.user_id, p.project_id;
