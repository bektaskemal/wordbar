//! The application service.
//!
//! Everything the D-Bus layer offers is implemented here, in plain synchronous
//! Rust with no zbus types in sight. Tests drive this directly, which is what
//! makes the "prove the backend before writing any UI" milestone possible.

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::SeedableRng;
use wordbar_core::{
    select_display_word, Error, FsrsScheduler, NewWord, Rating, Result, Review, ReviewCard,
    Scheduler, SelectionPolicy, Storage, Word, WordFilter, WordId, WordPatch, WordStatus,
};

/// Things that happened as a result of a call, for the caller to turn into
/// D-Bus signals. The engine itself knows nothing about signalling.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Effects {
    pub word_added: Option<WordId>,
    pub word_updated: Option<WordId>,
    pub vocabulary_changed: bool,
    pub current_word_changed: Option<Option<WordId>>,
}

impl Effects {
    fn added(id: WordId) -> Self {
        Self {
            word_added: Some(id),
            vocabulary_changed: true,
            ..Self::default()
        }
    }

    fn updated(id: WordId) -> Self {
        Self {
            word_updated: Some(id),
            vocabulary_changed: true,
            ..Self::default()
        }
    }

    fn merge(mut self, other: Effects) -> Self {
        self.word_added = self.word_added.or(other.word_added);
        self.word_updated = self.word_updated.or(other.word_updated);
        self.vocabulary_changed |= other.vocabulary_changed;
        self.current_word_changed = other.current_word_changed.or(self.current_word_changed);
        self
    }
}

/// A value plus the effects its computation produced.
#[derive(Debug, Clone)]
pub struct Outcome<T> {
    pub value: T,
    pub effects: Effects,
}

impl<T> Outcome<T> {
    fn new(value: T, effects: Effects) -> Self {
        Self { value, effects }
    }

    fn quiet(value: T) -> Self {
        Self {
            value,
            effects: Effects::default(),
        }
    }
}

/// Vocabulary counters for the manager's status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub active: usize,
    pub learned: usize,
}

pub struct Engine {
    store: Box<dyn Storage>,
    scheduler: Box<dyn Scheduler>,
    policy: SelectionPolicy,
    rng: StdRng,
}

impl Engine {
    pub fn new(store: Box<dyn Storage>) -> Self {
        Self {
            store,
            scheduler: Box::new(FsrsScheduler::new()),
            policy: SelectionPolicy::default(),
            rng: StdRng::from_entropy(),
        }
    }

    /// Deterministic variant for tests.
    pub fn with_seed(store: Box<dyn Storage>, seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            ..Self::new(store)
        }
    }

    pub fn set_policy(&mut self, policy: SelectionPolicy) {
        self.policy = policy;
    }

    // -- vocabulary -------------------------------------------------------

    /// Capture a term. Duplicates are reported, never silently created.
    pub fn add_word(&mut self, text: &str, now: DateTime<Utc>) -> Result<Outcome<Word>> {
        let word = self.store.add_word(NewWord::term(text), now)?;
        let id = word.id;
        Ok(Outcome::new(word, Effects::added(id)))
    }

    /// Import path: a term that may already arrive with dictionary data.
    pub fn add_word_full(&mut self, new: NewWord, now: DateTime<Utc>) -> Result<Outcome<Word>> {
        let word = self.store.add_word(new, now)?;
        let id = word.id;
        Ok(Outcome::new(word, Effects::added(id)))
    }

    pub fn get_word(&self, id: WordId) -> Result<Word> {
        self.store.get_word(id)?.ok_or(Error::UnknownWord(id))
    }

    pub fn find_by_term(&self, term: &str) -> Result<Option<Word>> {
        self.store.find_by_term(term)
    }

    pub fn update_word(
        &mut self,
        id: WordId,
        patch: WordPatch,
        now: DateTime<Utc>,
    ) -> Result<Outcome<Word>> {
        let word = self.store.update_word(id, patch, now)?;
        Ok(Outcome::new(word, Effects::updated(id)))
    }

    pub fn list_words(&self, filter: WordFilter, query: &str) -> Result<Vec<Word>> {
        if query.trim().is_empty() {
            self.store.list_words(filter)
        } else {
            self.store.search_words(query, filter)
        }
    }

    pub fn stats(&self) -> Result<Stats> {
        Ok(Stats {
            active: self.store.count_words(WordFilter::Active)?,
            learned: self.store.count_words(WordFilter::Learned)?,
        })
    }

    /// Archive a word. This is a vocabulary operation, not a review: FSRS is
    /// deliberately left untouched so a restore resumes where it left off.
    pub fn mark_learned(&mut self, id: WordId, now: DateTime<Utc>) -> Result<Outcome<Word>> {
        let was_current = self.store.current_word_id()? == Some(id);
        let word = self.store.mark_learned(id, now)?;
        let mut effects = Effects::updated(id);
        if was_current {
            effects.current_word_changed = Some(None);
        }
        Ok(Outcome::new(word, effects))
    }

    pub fn restore_word(&mut self, id: WordId, now: DateTime<Utc>) -> Result<Outcome<Word>> {
        let word = self.store.restore_word(id, now)?;
        Ok(Outcome::new(word, Effects::updated(id)))
    }

    pub fn delete_word(&mut self, id: WordId) -> Result<Outcome<()>> {
        let was_current = self.store.current_word_id()? == Some(id);
        self.store.delete_word(id)?;
        let mut effects = Effects {
            vocabulary_changed: true,
            ..Effects::default()
        };
        if was_current {
            effects.current_word_changed = Some(None);
        }
        Ok(Outcome::new((), effects))
    }

    // -- display ----------------------------------------------------------

    /// The word in the panel right now.
    ///
    /// Read-only in the steady state: it only writes when no word has ever
    /// been selected, or when the stored one has since been archived or
    /// deleted. Polling this (Waybar, a panel refresh) never rotates.
    pub fn current_word(&mut self, now: DateTime<Utc>) -> Result<Outcome<Option<Word>>> {
        if let Some(id) = self.store.current_word_id()? {
            match self.store.get_word(id)? {
                Some(word) if word.status == WordStatus::Active => {
                    return Ok(Outcome::quiet(Some(word)))
                }
                // Stale pointer: fall through and pick a fresh word.
                _ => {}
            }
        }
        self.advance_display(now)
    }

    /// Move to the next word. This is the only state-changing display call.
    pub fn advance_display(&mut self, now: DateTime<Utc>) -> Result<Outcome<Option<Word>>> {
        let current = self.store.current_word_id()?;
        let candidates = self.store.display_candidates()?;

        let selected = select_display_word(&candidates, current, now, &self.policy, &mut self.rng);

        match selected {
            None => {
                let changed = current.is_some();
                if changed {
                    self.store.set_current_word_id(None)?;
                }
                Ok(Outcome::new(
                    None,
                    Effects {
                        current_word_changed: changed.then_some(None),
                        ..Effects::default()
                    },
                ))
            }
            Some(id) => {
                self.store.set_current_word_id(Some(id))?;
                // Display is not a review; it only records that the word was
                // shown, which feeds the "don't repeat" part of the policy.
                self.store.touch_displayed(id, now)?;
                let word = self.get_word(id)?;
                Ok(Outcome::new(
                    Some(word),
                    Effects {
                        current_word_changed: Some(Some(id)),
                        ..Effects::default()
                    },
                ))
            }
        }
    }

    pub fn current_word_id(&self) -> Result<Option<WordId>> {
        self.store.current_word_id()
    }

    // -- review -----------------------------------------------------------

    /// Apply an explicit rating. Only this path touches FSRS.
    pub fn rate(
        &mut self,
        id: WordId,
        rating: Rating,
        now: DateTime<Utc>,
    ) -> Result<Outcome<ReviewCard>> {
        let word = self.get_word(id)?;
        let card = self
            .store
            .get_card(id)?
            .unwrap_or_else(|| ReviewCard::new(id, word.created_at));

        let next = self.scheduler.apply(&card, rating, now);
        self.store.save_card(&next)?;
        self.store.record_review(id, rating, now)?;

        Ok(Outcome::new(next, Effects::updated(id)))
    }

    /// Rate the current word and immediately move on, which is what every
    /// popup button does.
    pub fn rate_and_advance(
        &mut self,
        id: WordId,
        rating: Rating,
        now: DateTime<Utc>,
    ) -> Result<Outcome<Option<Word>>> {
        let effects = self.rate(id, rating, now)?.effects;
        let advanced = self.advance_display(now)?;
        Ok(Outcome::new(
            advanced.value,
            effects.merge(advanced.effects),
        ))
    }

    pub fn get_card(&self, id: WordId) -> Result<Option<ReviewCard>> {
        self.store.get_card(id)
    }

    pub fn list_reviews(&self, id: WordId) -> Result<Vec<Review>> {
        self.store.list_reviews(id)
    }
}
