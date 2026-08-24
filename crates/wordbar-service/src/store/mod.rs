//! SQLite implementation of [`wordbar_core::Storage`].
//!
//! This is the only code in Wordbar that touches the database. Neither the
//! GNOME extension nor `wordbarctl` ever opens `wordbar.db`.

pub mod migrations;

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use wordbar_core::{
    normalize_term, DisplayCandidate, Error, NewWord, Rating, Result, Review, ReviewCard, Storage,
    Word, WordFilter, WordId, WordPatch, WordStatus,
};

const META_CURRENT_WORD: &str = "current_word_id";

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open (creating if needed) the database at `path` and migrate it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Storage(format!("cannot create {}: {e}", parent.display())))?;
        }
        let conn = Connection::open(path).map_err(sql_err)?;
        Self::from_connection(conn)
    }

    /// An in-memory database. Used by tests and `--dry-run` style tooling.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(sql_err)?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        migrations::migrate(&mut conn).map_err(sql_err)?;
        Ok(Self { conn })
    }

    fn meta_get(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(sql_err)
    }

    fn meta_set(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, v],
            ),
            None => self
                .conn
                .execute("DELETE FROM meta WHERE key = ?1", params![key]),
        }
        .map(|_| ())
        .map_err(sql_err)
    }

    fn require_word(&self, id: WordId) -> Result<Word> {
        self.get_word(id)?.ok_or(Error::UnknownWord(id))
    }
}

const WORD_COLUMNS: &str = "id, term, normalized_term, definition, example, part_of_speech, \
                            phonetic, status, created_at, updated_at, last_displayed_at";

const CARD_COLUMNS: &str = "word_id, due, stability, difficulty, elapsed_days, scheduled_days, \
                            reps, lapses, state, last_review";

impl Storage for SqliteStore {
    fn add_word(&mut self, new: NewWord, now: DateTime<Utc>) -> Result<Word> {
        let normalized = normalize_term(&new.term)?;

        if let Some(existing) = self.find_by_term(&normalized.display)? {
            return Err(Error::DuplicateTerm(existing.term));
        }

        let tx = self.conn.transaction().map_err(sql_err)?;
        tx.execute(
            "INSERT INTO words (term, normalized_term, definition, example, part_of_speech,
                                phonetic, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?7)",
            params![
                normalized.display,
                normalized.key,
                blank_to_none(new.definition),
                blank_to_none(new.example),
                blank_to_none(new.part_of_speech),
                blank_to_none(new.phonetic),
                ts(now),
            ],
        )
        .map_err(|e| duplicate_or_sql(e, &normalized.display))?;

        let id = tx.last_insert_rowid();

        // Every word gets a card immediately, so the selector never has to
        // cope with a word that has no scheduling state.
        let card = ReviewCard::new(id, now);
        insert_card(&tx, &card)?;
        tx.commit().map_err(sql_err)?;

        self.require_word(id)
    }

    fn find_by_term(&self, term: &str) -> Result<Option<Word>> {
        let normalized = normalize_term(term)?;
        self.conn
            .query_row(
                &format!("SELECT {WORD_COLUMNS} FROM words WHERE normalized_term = ?1"),
                [normalized.key],
                row_to_word,
            )
            .optional()
            .map_err(sql_err)
    }

    fn get_word(&self, id: WordId) -> Result<Option<Word>> {
        self.conn
            .query_row(
                &format!("SELECT {WORD_COLUMNS} FROM words WHERE id = ?1"),
                [id],
                row_to_word,
            )
            .optional()
            .map_err(sql_err)
    }

    fn update_word(&mut self, id: WordId, patch: WordPatch, now: DateTime<Utc>) -> Result<Word> {
        self.require_word(id)?;
        if patch.is_empty() {
            return self.require_word(id);
        }

        if let Some(term) = &patch.term {
            let normalized = normalize_term(term)?;
            if let Some(other) = self.find_by_term(&normalized.display)? {
                if other.id != id {
                    return Err(Error::DuplicateTerm(other.term));
                }
            }
            self.conn
                .execute(
                    "UPDATE words SET term = ?2, normalized_term = ?3 WHERE id = ?1",
                    params![id, normalized.display, normalized.key],
                )
                .map_err(|e| duplicate_or_sql(e, &normalized.display))?;
        }

        for (column, value) in [
            ("definition", &patch.definition),
            ("example", &patch.example),
            ("part_of_speech", &patch.part_of_speech),
            ("phonetic", &patch.phonetic),
        ] {
            if let Some(value) = value {
                self.conn
                    .execute(
                        &format!("UPDATE words SET {column} = ?2 WHERE id = ?1"),
                        params![id, blank_to_none(value.clone())],
                    )
                    .map_err(sql_err)?;
            }
        }

        self.conn
            .execute(
                "UPDATE words SET updated_at = ?2 WHERE id = ?1",
                params![id, ts(now)],
            )
            .map_err(sql_err)?;

        self.require_word(id)
    }

    fn set_status(&mut self, id: WordId, status: WordStatus, now: DateTime<Utc>) -> Result<Word> {
        self.require_word(id)?;
        self.conn
            .execute(
                "UPDATE words SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, status.as_str(), ts(now)],
            )
            .map_err(sql_err)?;

        // A learned word must not stay the panel word.
        if status == WordStatus::Learned && self.current_word_id()? == Some(id) {
            self.set_current_word_id(None)?;
        }
        self.require_word(id)
    }

    fn delete_word(&mut self, id: WordId) -> Result<()> {
        // Cards and reviews go with it via ON DELETE CASCADE.
        let affected = self
            .conn
            .execute("DELETE FROM words WHERE id = ?1", [id])
            .map_err(sql_err)?;
        if affected == 0 {
            return Err(Error::UnknownWord(id));
        }
        if self.current_word_id()? == Some(id) {
            self.set_current_word_id(None)?;
        }
        Ok(())
    }

    fn list_words(&self, filter: WordFilter) -> Result<Vec<Word>> {
        let sql = format!(
            "SELECT {WORD_COLUMNS} FROM words {} ORDER BY normalized_term COLLATE NOCASE",
            status_clause(filter)
        );
        let mut stmt = self.conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt.query_map([], row_to_word).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    fn search_words(&self, query: &str, filter: WordFilter) -> Result<Vec<Word>> {
        let needle = format!("%{}%", query.trim().to_lowercase());
        let sql = format!(
            "SELECT {WORD_COLUMNS} FROM words
             {} {} (normalized_term LIKE ?1
                    OR LOWER(COALESCE(definition, '')) LIKE ?1
                    OR LOWER(COALESCE(example, '')) LIKE ?1)
             ORDER BY normalized_term COLLATE NOCASE",
            status_clause(filter),
            if filter == WordFilter::All {
                "WHERE"
            } else {
                "AND"
            },
        );
        let mut stmt = self.conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt.query_map([needle], row_to_word).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    fn count_words(&self, filter: WordFilter) -> Result<usize> {
        let sql = format!("SELECT COUNT(*) FROM words {}", status_clause(filter));
        self.conn
            .query_row(&sql, [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(sql_err)
    }

    fn get_card(&self, word_id: WordId) -> Result<Option<ReviewCard>> {
        self.conn
            .query_row(
                &format!("SELECT {CARD_COLUMNS} FROM cards WHERE word_id = ?1"),
                [word_id],
                row_to_card,
            )
            .optional()
            .map_err(sql_err)
    }

    fn save_card(&mut self, card: &ReviewCard) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO cards (word_id, due, stability, difficulty, elapsed_days,
                                    scheduled_days, reps, lapses, state, last_review)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (word_id) DO UPDATE SET
                     due = excluded.due, stability = excluded.stability,
                     difficulty = excluded.difficulty, elapsed_days = excluded.elapsed_days,
                     scheduled_days = excluded.scheduled_days, reps = excluded.reps,
                     lapses = excluded.lapses, state = excluded.state,
                     last_review = excluded.last_review",
                params![
                    card.word_id,
                    ts(card.due),
                    card.stability,
                    card.difficulty,
                    card.elapsed_days,
                    card.scheduled_days,
                    card.reps,
                    card.lapses,
                    card.state,
                    ts(card.last_review),
                ],
            )
            .map(|_| ())
            .map_err(sql_err)
    }

    fn record_review(
        &mut self,
        word_id: WordId,
        rating: Rating,
        reviewed_at: DateTime<Utc>,
    ) -> Result<Review> {
        self.conn
            .execute(
                "INSERT INTO reviews (word_id, rating, reviewed_at) VALUES (?1, ?2, ?3)",
                params![word_id, rating.as_str(), ts(reviewed_at)],
            )
            .map_err(sql_err)?;
        Ok(Review {
            id: self.conn.last_insert_rowid(),
            word_id,
            rating,
            reviewed_at,
        })
    }

    fn list_reviews(&self, word_id: WordId) -> Result<Vec<Review>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, word_id, rating, reviewed_at FROM reviews
                 WHERE word_id = ?1 ORDER BY reviewed_at, id",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([word_id], |row| {
                Ok(Review {
                    id: row.get(0)?,
                    word_id: row.get(1)?,
                    rating: Rating::parse(&row.get::<_, String>(2)?).unwrap_or(Rating::Again),
                    reviewed_at: parse_ts(row, 3)?,
                })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    fn display_candidates(&self) -> Result<Vec<DisplayCandidate>> {
        let sql = format!(
            "SELECT {}, {} FROM words
             JOIN cards ON cards.word_id = words.id
             WHERE words.status = 'active'",
            WORD_COLUMNS
                .split(", ")
                .map(|c| format!("words.{c}"))
                .collect::<Vec<_>>()
                .join(", "),
            CARD_COLUMNS
                .split(", ")
                .map(|c| format!("cards.{c}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut stmt = self.conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DisplayCandidate {
                    word: row_to_word(row)?,
                    card: row_to_card_offset(row, 11)?,
                })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    fn touch_displayed(&mut self, id: WordId, now: DateTime<Utc>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE words SET last_displayed_at = ?2 WHERE id = ?1",
                params![id, ts(now)],
            )
            .map(|_| ())
            .map_err(sql_err)
    }

    fn current_word_id(&self) -> Result<Option<WordId>> {
        Ok(self
            .meta_get(META_CURRENT_WORD)?
            .and_then(|v| v.parse().ok()))
    }

    fn set_current_word_id(&mut self, id: Option<WordId>) -> Result<()> {
        self.meta_set(META_CURRENT_WORD, id.map(|i| i.to_string()).as_deref())
    }
}

fn status_clause(filter: WordFilter) -> &'static str {
    match filter {
        WordFilter::Active => "WHERE status = 'active'",
        WordFilter::Learned => "WHERE status = 'learned'",
        WordFilter::All => "",
    }
}

/// ISO 8601 UTC with millisecond precision, so string ordering matches time
/// ordering (ADR-0004).
fn ts(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_ts(row: &Row, idx: usize) -> rusqlite::Result<DateTime<Utc>> {
    let raw: String = row.get(idx)?;
    DateTime::parse_from_rfc3339(&raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
        })
}

fn parse_ts_opt(row: &Row, idx: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    match row.get::<_, Option<String>>(idx)? {
        None => Ok(None),
        Some(raw) => DateTime::parse_from_rfc3339(&raw)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    idx,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            }),
    }
}

fn row_to_word(row: &Row) -> rusqlite::Result<Word> {
    Ok(Word {
        id: row.get(0)?,
        term: row.get(1)?,
        normalized_term: row.get(2)?,
        definition: row.get(3)?,
        example: row.get(4)?,
        part_of_speech: row.get(5)?,
        phonetic: row.get(6)?,
        status: WordStatus::parse(&row.get::<_, String>(7)?).unwrap_or(WordStatus::Active),
        created_at: parse_ts(row, 8)?,
        updated_at: parse_ts(row, 9)?,
        last_displayed_at: parse_ts_opt(row, 10)?,
    })
}

fn row_to_card(row: &Row) -> rusqlite::Result<ReviewCard> {
    row_to_card_offset(row, 0)
}

fn row_to_card_offset(row: &Row, offset: usize) -> rusqlite::Result<ReviewCard> {
    Ok(ReviewCard {
        word_id: row.get(offset)?,
        due: parse_ts(row, offset + 1)?,
        stability: row.get(offset + 2)?,
        difficulty: row.get(offset + 3)?,
        elapsed_days: row.get(offset + 4)?,
        scheduled_days: row.get(offset + 5)?,
        reps: row.get(offset + 6)?,
        lapses: row.get(offset + 7)?,
        state: row.get(offset + 8)?,
        last_review: parse_ts(row, offset + 9)?,
    })
}

fn insert_card(tx: &rusqlite::Transaction, card: &ReviewCard) -> Result<()> {
    tx.execute(
        "INSERT INTO cards (word_id, due, stability, difficulty, elapsed_days,
                            scheduled_days, reps, lapses, state, last_review)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            card.word_id,
            ts(card.due),
            card.stability,
            card.difficulty,
            card.elapsed_days,
            card.scheduled_days,
            card.reps,
            card.lapses,
            card.state,
            ts(card.last_review),
        ],
    )
    .map(|_| ())
    .map_err(sql_err)
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn sql_err(err: rusqlite::Error) -> Error {
    Error::Storage(err.to_string())
}

/// Turn a UNIQUE violation on `normalized_term` into a domain-level duplicate.
fn duplicate_or_sql(err: rusqlite::Error, term: &str) -> Error {
    match &err {
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Error::DuplicateTerm(term.to_string())
        }
        _ => sql_err(err),
    }
}
