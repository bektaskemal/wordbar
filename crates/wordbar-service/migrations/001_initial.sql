-- Wordbar schema v1.
--
-- Timestamps are ISO 8601 UTC strings (ADR-0004). The service is the only
-- writer; clients reach the database exclusively through D-Bus.

CREATE TABLE words (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    term              TEXT    NOT NULL,
    normalized_term   TEXT    NOT NULL UNIQUE,
    definition        TEXT,
    example           TEXT,
    part_of_speech    TEXT,
    phonetic          TEXT,
    status            TEXT    NOT NULL DEFAULT 'active'
                              CHECK (status IN ('active', 'learned')),
    created_at        TEXT    NOT NULL,
    updated_at        TEXT    NOT NULL,
    last_displayed_at TEXT
);

CREATE INDEX idx_words_status ON words (status);
CREATE INDEX idx_words_normalized ON words (normalized_term);

-- FSRS state, one row per word, created together with the word.
CREATE TABLE cards (
    word_id        INTEGER PRIMARY KEY REFERENCES words (id) ON DELETE CASCADE,
    due            TEXT    NOT NULL,
    stability      REAL    NOT NULL DEFAULT 0,
    difficulty     REAL    NOT NULL DEFAULT 0,
    elapsed_days   INTEGER NOT NULL DEFAULT 0,
    scheduled_days INTEGER NOT NULL DEFAULT 0,
    reps           INTEGER NOT NULL DEFAULT 0,
    lapses         INTEGER NOT NULL DEFAULT 0,
    state          INTEGER NOT NULL DEFAULT 0,
    last_review    TEXT    NOT NULL
);

CREATE INDEX idx_cards_due ON cards (due);

-- Full rating history, kept from the beginning for undo, debugging and any
-- future scheduler migration.
CREATE TABLE reviews (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    word_id     INTEGER NOT NULL REFERENCES words (id) ON DELETE CASCADE,
    rating      TEXT    NOT NULL CHECK (rating IN ('again', 'know')),
    reviewed_at TEXT    NOT NULL
);

CREATE INDEX idx_reviews_word ON reviews (word_id);

-- Small key/value store for schema version and service state such as the
-- currently displayed word.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT
);
