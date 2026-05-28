-- Embeddings are now stored in LanceDB, not SQLite.
-- Drop the BLOB column and its model tag; keep has_embedding / indexed_in_vectors
-- as status flags (used by audit and worker re-index queries).
ALTER TABLE chunks DROP COLUMN embedding;
ALTER TABLE chunks DROP COLUMN embedding_model;

-- Remove partial indexes that referenced dropped columns (they won't exist
-- post-migration since the columns are gone; recreate without them).
DROP INDEX IF EXISTS idx_chunks_needs_embed;
DROP INDEX IF EXISTS idx_chunks_needs_vec_index;

CREATE INDEX idx_chunks_needs_embed
    ON chunks(has_embedding) WHERE has_embedding = 0;

CREATE INDEX idx_chunks_needs_vec_index
    ON chunks(indexed_in_vectors) WHERE indexed_in_vectors = 0;
