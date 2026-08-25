-- Schema v2: keep every dictionary sense, not just the one on show.
--
-- The chosen sense stays denormalized onto `words` so the panel, the CLI and
-- Waybar keep reading a single definition and need know nothing about senses.

CREATE TABLE senses (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    word_id        INTEGER NOT NULL REFERENCES words (id) ON DELETE CASCADE,
    -- Rank within the word, 0 first, as decided by wordbar-core::rank_senses.
    position       INTEGER NOT NULL,
    part_of_speech TEXT,
    definition     TEXT    NOT NULL,
    example        TEXT,
    UNIQUE (word_id, position)
);

CREATE INDEX idx_senses_word ON senses (word_id);

-- Existing words keep working with no senses recorded: enrichment fills them
-- in on the next lookup, and a word edited by hand simply has none.
