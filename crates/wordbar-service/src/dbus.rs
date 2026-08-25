//! The `org.wordbar.Wordbar` D-Bus interface.
//!
//! This layer is a thin translation of [`crate::engine::Engine`]: it converts
//! domain values to D-Bus dictionaries, turns domain errors into named D-Bus
//! errors, and emits signals for the effects the engine reports. No business
//! logic lives here.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use wordbar_core::{
    DictionaryProvider, Error as CoreError, Rating, Word, WordFilter, WordId, WordPatch,
};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{interface, Connection, DBusError};

use crate::engine::{Effects, Engine, Outcome};
use crate::exchange::{self, Format};

pub const BUS_NAME: &str = "org.wordbar.Wordbar";
pub const OBJECT_PATH: &str = "/org/wordbar/Wordbar";
pub const INTERFACE: &str = "org.wordbar.Wordbar";

/// A word rendered for the wire. `a{sv}` rather than a fixed struct so fields
/// can be added without breaking the GJS and CLI clients.
pub type WordDict = HashMap<String, OwnedValue>;

/// Named errors, so clients can react to a duplicate differently from a
/// storage failure without parsing English.
#[derive(Debug, DBusError)]
#[zbus(prefix = "org.wordbar.Error")]
pub enum ServiceError {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The term is already in the vocabulary.
    Duplicate(String),
    /// No such word id.
    NotFound(String),
    /// The text is not a usable vocabulary term.
    InvalidTerm(String),
    /// The vocabulary has no active words.
    Empty(String),
    /// Anything that went wrong below the domain layer.
    Failed(String),
}

// NOTE: zbus's `#[interface]` macro matches on the literal `Result<T, E>` return
// type, so methods below spell it out instead of using an alias.

impl From<CoreError> for ServiceError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::DuplicateTerm(_) => Self::Duplicate(err.to_string()),
            CoreError::UnknownWord(_) => Self::NotFound(err.to_string()),
            CoreError::EmptyTerm
            | CoreError::TermTooLong { .. }
            | CoreError::TooManyWords { .. }
            | CoreError::InvalidCharacters => Self::InvalidTerm(err.to_string()),
            CoreError::EmptyVocabulary => Self::Empty(err.to_string()),
            other => Self::Failed(other.to_string()),
        }
    }
}

pub struct WordbarService {
    engine: Arc<Mutex<Engine>>,
    dictionary: Arc<dyn DictionaryProvider>,
    connection: Connection,
    /// Woken by `Quit`, so the service can be stopped from the panel.
    shutdown: Arc<tokio::sync::Notify>,
}

impl WordbarService {
    pub fn new(
        engine: Arc<Mutex<Engine>>,
        dictionary: Arc<dyn DictionaryProvider>,
        connection: Connection,
        shutdown: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            engine,
            dictionary,
            connection,
            shutdown,
        }
    }

    fn emitter(&self) -> zbus::Result<SignalEmitter<'static>> {
        SignalEmitter::new(&self.connection, OBJECT_PATH)
    }

    /// Publish whatever the engine reported.
    async fn publish(&self, effects: &Effects) {
        let emitter = match self.emitter() {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(%err, "cannot build signal emitter");
                return;
            }
        };
        if let Some(id) = effects.word_added {
            let _ = Self::word_added(&emitter, id).await;
        }
        if let Some(id) = effects.word_updated {
            let _ = Self::word_updated(&emitter, id).await;
        }
        if effects.vocabulary_changed {
            let _ = Self::vocabulary_changed(&emitter).await;
        }
        if let Some(current) = effects.current_word_changed {
            let dict = match current {
                Some(id) => {
                    let engine = self.engine.lock().await;
                    let now = Utc::now();
                    engine
                        .get_word(id)
                        .map(|w| word_dict_with_card(&engine, &w, now))
                        .unwrap_or_default()
                }
                None => WordDict::new(),
            };
            let _ = Self::current_word_changed(&emitter, dict).await;
        }
    }

    /// Apply an outcome: publish its effects, return its value.
    async fn settle<T>(&self, outcome: Outcome<T>) -> T {
        self.publish(&outcome.effects).await;
        outcome.value
    }

    /// Render a word for the wire, or the empty dictionary for "no word".
    async fn dict(&self, word: Option<&Word>) -> WordDict {
        match word {
            None => WordDict::new(),
            Some(word) => {
                let engine = self.engine.lock().await;
                word_dict_with_card(&engine, word, Utc::now())
            }
        }
    }

    /// Kick off best-effort enrichment. Never awaited by the caller, so a slow
    /// or failing network cannot delay a capture.
    fn spawn_enrichment(&self, id: WordId, term: String) {
        let engine = Arc::clone(&self.engine);
        let dictionary = Arc::clone(&self.dictionary);
        let connection = self.connection.clone();

        tokio::spawn(async move {
            let entry = match dictionary.lookup(&term).await {
                Ok(Some(entry)) => entry,
                Ok(None) => {
                    tracing::debug!(%term, "no dictionary entry");
                    return;
                }
                Err(err) => {
                    tracing::info!(%term, %err, "dictionary lookup failed; word kept as-is");
                    return;
                }
            };

            let effects = {
                let mut engine = engine.lock().await;
                // The word may have been deleted while we were on the network.
                match engine.apply_enrichment(id, &entry, Utc::now()) {
                    Ok(outcome) => outcome.effects,
                    Err(err) => {
                        tracing::info!(id, %err, "enrichment update skipped");
                        return;
                    }
                }
            };

            if let (Ok(emitter), Some(updated)) = (
                SignalEmitter::new(&connection, OBJECT_PATH),
                effects.word_updated,
            ) {
                let _ = Self::word_updated(&emitter, updated).await;
                let _ = Self::vocabulary_changed(&emitter).await;
            }
        });
    }
}

#[interface(name = "org.wordbar.Wordbar")]
impl WordbarService {
    /// Capture a term. Returns the stored word immediately; dictionary data
    /// arrives later via `WordUpdated`.
    async fn add_word(&self, text: &str) -> Result<WordDict, ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.add_word(text, Utc::now())?
        };
        let word = outcome.value.clone();
        self.publish(&outcome.effects).await;
        self.spawn_enrichment(word.id, word.term.clone());
        Ok(self.dict(Some(&word)).await)
    }

    /// The panel word. Read-only in the steady state — safe to poll.
    async fn get_current_word(&self) -> Result<WordDict, ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.current_word(Utc::now())?
        };
        let word = self.settle(outcome).await;
        Ok(self.dict(word.as_ref()).await)
    }

    /// Rotate to the next word. State-changing; do not poll this.
    async fn advance_display(&self) -> Result<WordDict, ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.advance_display(Utc::now())?
        };
        let word = self.settle(outcome).await;
        Ok(self.dict(word.as_ref()).await)
    }

    async fn get_word_details(&self, id: i64) -> Result<WordDict, ServiceError> {
        let engine = self.engine.lock().await;
        let word = engine.get_word(id)?;
        let mut dict = word_dict_with_card(&engine, &word, Utc::now());
        if let Some(card) = engine.get_card(id)? {
            insert(&mut dict, "due", card.due.to_rfc3339());
            insert(&mut dict, "reps", card.reps as i64);
            insert(&mut dict, "lapses", card.lapses as i64);
            insert(&mut dict, "is_new", card.is_new());
        }
        Ok(dict)
    }

    /// Resolve a term to a word, using the same normalization as capture.
    /// Returns an empty dictionary when the term is not in the vocabulary.
    async fn find_word(&self, term: &str) -> Result<WordDict, ServiceError> {
        let engine = self.engine.lock().await;
        let now = Utc::now();
        Ok(engine
            .find_by_term(term)?
            .as_ref()
            .map(|w| word_dict_with_card(&engine, w, now))
            .unwrap_or_default())
    }

    /// The user could not recall the word. Records FSRS `Again` and advances.
    async fn rate_again(&self, id: i64) -> Result<WordDict, ServiceError> {
        self.rate(id, Rating::Again).await
    }

    /// The user recalled the word. Records FSRS `Good` and advances.
    async fn rate_know(&self, id: i64) -> Result<WordDict, ServiceError> {
        self.rate(id, Rating::Know).await
    }

    /// Archive a word: stop resurfacing it. Not an FSRS review.
    async fn mark_learned(&self, id: i64) -> Result<WordDict, ServiceError> {
        let (marked, advanced) = {
            let mut engine = self.engine.lock().await;
            let now = Utc::now();
            let marked = engine.mark_learned(id, now)?;
            let advanced = engine.advance_display(now)?;
            (marked, advanced)
        };
        self.publish(&marked.effects).await;
        let word = self.settle(advanced).await;
        Ok(self.dict(word.as_ref()).await)
    }

    /// Bring an archived word back into rotation with its FSRS state intact.
    async fn restore_word(&self, id: i64) -> Result<WordDict, ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.restore_word(id, Utc::now())?
        };
        let word = self.settle(outcome).await;
        Ok(self.dict(Some(&word)).await)
    }

    async fn delete_word(&self, id: i64) -> Result<(), ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.delete_word(id)?
        };
        self.settle(outcome).await;
        Ok(())
    }

    /// Edit a word. Keys: term, definition, example, part_of_speech, phonetic.
    /// An empty string clears a field.
    async fn update_word(
        &self,
        id: i64,
        fields: HashMap<String, String>,
    ) -> Result<WordDict, ServiceError> {
        let text = |key: &str| fields.get(key).map(|v| Some(v.clone()));
        let patch = WordPatch {
            term: fields.get("term").cloned().filter(|t| !t.trim().is_empty()),
            definition: text("definition"),
            example: text("example"),
            part_of_speech: text("part_of_speech"),
            phonetic: text("phonetic"),
            // Not editable by hand: it is dictionary metadata, not content.
            audio_url: None,
        };
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.update_word(id, patch, Utc::now())?
        };
        let word = self.settle(outcome).await;
        Ok(self.dict(Some(&word)).await)
    }

    /// Every sense recorded for a word, best-ranked first. Each carries
    /// `selected`, so a chooser can mark the one on show.
    async fn list_senses(&self, id: i64) -> Result<Vec<WordDict>, ServiceError> {
        let engine = self.engine.lock().await;
        let word = engine.get_word(id)?;
        Ok(engine
            .list_senses(id)?
            .iter()
            .map(|sense| {
                let mut dict = WordDict::new();
                insert(&mut dict, "id", sense.id);
                insert(&mut dict, "word_id", sense.word_id);
                insert(&mut dict, "position", sense.position);
                insert(&mut dict, "definition", sense.definition.clone());
                insert(
                    &mut dict,
                    "part_of_speech",
                    sense.part_of_speech.clone().unwrap_or_default(),
                );
                insert(
                    &mut dict,
                    "example",
                    sense.example.clone().unwrap_or_default(),
                );
                insert(&mut dict, "summary", sense.summary());
                insert(&mut dict, "selected", sense.is_selected(&word));
                dict
            })
            .collect())
    }

    /// Show a different sense of a word from now on. Switching is how a wrong
    /// pick is corrected, so it persists rather than previewing.
    async fn select_sense(&self, sense_id: i64) -> Result<WordDict, ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.select_sense(sense_id, Utc::now())?
        };
        let word = self.settle(outcome).await;
        Ok(self.dict(Some(&word)).await)
    }

    /// List the vocabulary. `filter` is `active`, `learned` or `all`;
    /// `query` is a substring match over term, definition and example.
    async fn list_words(&self, filter: &str, query: &str) -> Result<Vec<WordDict>, ServiceError> {
        let filter = WordFilter::parse(filter)
            .ok_or_else(|| ServiceError::InvalidTerm(format!("unknown filter \"{filter}\"")))?;
        let engine = self.engine.lock().await;
        let now = Utc::now();
        Ok(engine
            .list_words(filter, query)?
            .iter()
            .map(|w| word_dict_with_card(&engine, w, now))
            .collect())
    }

    /// `(active, learned)` counts for the manager's status line.
    async fn get_stats(&self) -> Result<(u32, u32), ServiceError> {
        let engine = self.engine.lock().await;
        let stats = engine.stats()?;
        Ok((stats.active as u32, stats.learned as u32))
    }

    /// Portable vocabulary as JSON or CSV. Never includes scheduler state.
    async fn export_vocabulary(&self, filter: &str, format: &str) -> Result<String, ServiceError> {
        let filter = WordFilter::parse(filter)
            .ok_or_else(|| ServiceError::InvalidTerm(format!("unknown filter \"{filter}\"")))?;
        let format = Format::parse(format)
            .ok_or_else(|| ServiceError::InvalidTerm("format must be json or csv".into()))?;
        let engine = self.engine.lock().await;
        Ok(exchange::export(&engine, filter, format)?)
    }

    /// Import portable vocabulary. Returns `(added, duplicates, invalid)`.
    async fn import_vocabulary(
        &self,
        data: &str,
        format: &str,
    ) -> Result<(u32, u32, u32), ServiceError> {
        let format = Format::parse(format)
            .ok_or_else(|| ServiceError::InvalidTerm("format must be json or csv".into()))?;
        let report = {
            let mut engine = self.engine.lock().await;
            exchange::import(&mut engine, data, format, Utc::now())?
        };
        self.publish(&Effects {
            vocabulary_changed: report.added > 0,
            ..Effects::default()
        })
        .await;
        Ok((
            report.added as u32,
            report.duplicates as u32,
            report.invalid as u32,
        ))
    }

    /// Launch the vocabulary manager.
    async fn open_vocabulary(&self) -> Result<(), ServiceError> {
        spawn_app(&["--page", "vocabulary"])
    }

    /// Open the manager with this word's editor already showing, so a word
    /// enrichment could not complete can be filled in without searching.
    async fn open_word_editor(&self, id: i64) -> Result<(), ServiceError> {
        let engine = self.engine.lock().await;
        engine.get_word(id)?;
        drop(engine);
        spawn_app(&["--edit", &id.to_string()])
    }

    /// Launch the manager on its preferences page.
    async fn open_preferences(&self) -> Result<(), ServiceError> {
        spawn_app(&["--page", "preferences"])
    }

    /// Launch the manager showing only words that still need a definition.
    async fn open_incomplete(&self) -> Result<(), ServiceError> {
        spawn_app(&["--incomplete"])
    }

    /// How many active words still have no definition, for the popup's hint.
    async fn count_incomplete(&self) -> Result<u32, ServiceError> {
        let engine = self.engine.lock().await;
        Ok(engine.count_incomplete()? as u32)
    }

    /// Look a word up again, replacing its senses, phonetic and recording.
    ///
    /// Awaited rather than spawned, unlike enrichment after a capture: the
    /// caller asked for this and wants to know whether it worked. Needed to
    /// backfill words captured before senses were stored.
    async fn refresh_word(&self, id: i64) -> Result<u32, ServiceError> {
        let term = {
            let engine = self.engine.lock().await;
            engine.get_word(id)?.term.clone()
        };

        let entry = self
            .dictionary
            .lookup(&term)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("no dictionary entry for {term}")))?;

        let count = entry.senses.len() as u32;
        let effects = {
            let mut engine = self.engine.lock().await;
            engine.apply_enrichment(id, &entry, Utc::now())?.effects
        };
        self.publish(&effects).await;
        Ok(count)
    }

    /// Pronounce a word: its dictionary recording if there is one, otherwise
    /// speech synthesis. Playback happens here, never in the Shell.
    async fn speak(&self, id: i64) -> Result<(), ServiceError> {
        let (term, audio_url) = {
            let engine = self.engine.lock().await;
            let word = engine.get_word(id)?;
            (word.term.clone(), word.audio_url.clone())
        };
        crate::speech::speak(&term, audio_url.as_deref()).await?;
        Ok(())
    }

    /// Stop the service.
    ///
    /// It is D-Bus activated, so the next call starts it again; this frees its
    /// memory rather than turning Wordbar off for good. Disabling the
    /// extension is what does that.
    async fn quit(&self) -> Result<(), ServiceError> {
        tracing::info!("quit requested");
        self.shutdown.notify_one();
        Ok(())
    }

    /// Liveness check used by the CLI and the extension's retry loop.
    async fn ping(&self) -> Result<String, ServiceError> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }

    #[zbus(signal)]
    async fn current_word_changed(emitter: &SignalEmitter<'_>, word: WordDict) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn word_added(emitter: &SignalEmitter<'_>, id: i64) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn word_updated(emitter: &SignalEmitter<'_>, id: i64) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn vocabulary_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

impl WordbarService {
    async fn rate(&self, id: i64, rating: Rating) -> Result<WordDict, ServiceError> {
        let outcome = {
            let mut engine = self.engine.lock().await;
            engine.rate_and_advance(id, rating, Utc::now())?
        };
        let word = self.settle(outcome).await;
        Ok(self.dict(word.as_ref()).await)
    }
}

/// Start `wordbar-app` detached. Failing to launch the manager must not take
/// down the service, so this only ever reports an error to the caller.
fn spawn_app(args: &[&str]) -> Result<(), ServiceError> {
    let binary = std::env::var("WORDBAR_APP").unwrap_or_else(|_| "wordbar-app".to_string());
    let child = std::process::Command::new(&binary)
        .args(args)
        .spawn()
        .map_err(|e| ServiceError::Failed(format!("cannot start {binary}: {e}")))?;

    // Reap it. The service outlives every manager window it starts, so without
    // this each launch leaves a zombie behind for the rest of the session.
    tokio::spawn(async move {
        let mut child = child;
        let _ = tokio::task::spawn_blocking(move || child.wait()).await;
    });

    Ok(())
}

fn insert(dict: &mut WordDict, key: &str, value: impl Into<Value<'static>>) {
    if let Ok(owned) = OwnedValue::try_from(value.into()) {
        dict.insert(key.to_string(), owned);
    }
}

/// Render a word together with its scheduling state.
///
/// The panel shows a dot when a word is due, so `is_due` travels with every
/// word rather than only with the detailed lookup.
fn word_dict_with_card(engine: &Engine, word: &Word, now: DateTime<Utc>) -> WordDict {
    let mut dict = word_dict(word);
    if let Ok(Some(card)) = engine.get_card(word.id) {
        insert(&mut dict, "is_due", card.is_due(now));
        insert(&mut dict, "due", card.due.to_rfc3339());
    }
    dict
}

/// Render a word for the wire. Absent optional fields become empty strings so
/// clients never have to test for key presence.
pub fn word_dict(word: &Word) -> WordDict {
    let mut dict = WordDict::new();
    insert(&mut dict, "id", word.id);
    insert(&mut dict, "term", word.term.clone());
    insert(&mut dict, "normalized_term", word.normalized_term.clone());
    insert(&mut dict, "definition", opt(&word.definition));
    insert(&mut dict, "example", opt(&word.example));
    insert(&mut dict, "part_of_speech", opt(&word.part_of_speech));
    insert(&mut dict, "phonetic", opt(&word.phonetic));
    insert(&mut dict, "status", word.status.as_str().to_string());
    insert(&mut dict, "created_at", word.created_at.to_rfc3339());
    insert(&mut dict, "updated_at", word.updated_at.to_rfc3339());
    insert(
        &mut dict,
        "last_displayed_at",
        word.last_displayed_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_default(),
    );
    insert(&mut dict, "incomplete", word.is_incomplete());
    insert(
        &mut dict,
        "has_audio",
        word.audio_url
            .as_deref()
            .is_some_and(|u| !u.trim().is_empty()),
    );
    dict
}

fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}
