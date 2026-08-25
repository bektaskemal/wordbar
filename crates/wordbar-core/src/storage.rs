//! Storage abstraction.
//!
//! The core never talks to SQLite directly. `wordbar-service` provides the one
//! and only implementation and is the single writer to the database.

use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::{Rating, Review, ReviewCard, Sense, SenseId, Word, WordId, WordStatus};
use crate::select::DisplayCandidate;

/// Everything needed to create a vocabulary entry.
#[derive(Debug, Clone, Default)]
pub struct NewWord {
    pub term: String,
    pub definition: Option<String>,
    pub example: Option<String>,
    pub part_of_speech: Option<String>,
    pub phonetic: Option<String>,
    pub audio_url: Option<String>,
}

impl NewWord {
    pub fn term(term: impl Into<String>) -> Self {
        Self {
            term: term.into(),
            ..Self::default()
        }
    }
}

/// Sparse update of an existing word.
///
/// `None` leaves a field untouched; `Some(None)` clears it. This distinction
/// matters because the vocabulary manager must be able to erase a wrong
/// definition, not just replace it.
#[derive(Debug, Clone, Default)]
pub struct WordPatch {
    pub term: Option<String>,
    pub definition: Option<Option<String>>,
    pub example: Option<Option<String>>,
    pub part_of_speech: Option<Option<String>>,
    pub phonetic: Option<Option<String>>,
    pub audio_url: Option<Option<String>>,
}

impl WordPatch {
    pub fn is_empty(&self) -> bool {
        self.term.is_none()
            && self.definition.is_none()
            && self.example.is_none()
            && self.part_of_speech.is_none()
            && self.phonetic.is_none()
            && self.audio_url.is_none()
    }
}

/// A sense about to be stored, before it has an id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewSense {
    pub part_of_speech: Option<String>,
    pub definition: String,
    pub example: Option<String>,
}

/// Which slice of the vocabulary to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WordFilter {
    /// In rotation: not archived by hand.
    #[default]
    Active,
    /// Archived by hand.
    Learned,
    All,
    /// In rotation and not yet known well: the working set.
    Learning,
    /// Known: archived by hand, or an interval past
    /// [`crate::model::KNOWN_INTERVAL_DAYS`].
    Known,
}

impl WordFilter {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "learned" => Some(Self::Learned),
            "learning" => Some(Self::Learning),
            "known" => Some(Self::Known),
            "all" | "" => Some(Self::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Learned => "learned",
            Self::Learning => "learning",
            Self::Known => "known",
            Self::All => "all",
        }
    }

    /// The stored-status filter this maps onto; the schedule decides the rest.
    pub fn status_scope(self) -> Self {
        match self {
            Self::Learning => Self::Active,
            Self::Known => Self::All,
            other => other,
        }
    }

    /// True when the filter has to consult the schedule, not just the status.
    pub fn needs_schedule(self) -> bool {
        matches!(self, Self::Learning | Self::Known)
    }
}

/// Persistent vocabulary state.
pub trait Storage: Send {
    fn add_word(&mut self, new: NewWord, now: DateTime<Utc>) -> Result<Word>;
    fn find_by_term(&self, term: &str) -> Result<Option<Word>>;
    fn get_word(&self, id: WordId) -> Result<Option<Word>>;
    fn update_word(&mut self, id: WordId, patch: WordPatch, now: DateTime<Utc>) -> Result<Word>;

    fn set_status(&mut self, id: WordId, status: WordStatus, now: DateTime<Utc>) -> Result<Word>;
    fn mark_learned(&mut self, id: WordId, now: DateTime<Utc>) -> Result<Word> {
        self.set_status(id, WordStatus::Learned, now)
    }
    fn restore_word(&mut self, id: WordId, now: DateTime<Utc>) -> Result<Word> {
        self.set_status(id, WordStatus::Active, now)
    }
    fn delete_word(&mut self, id: WordId) -> Result<()>;

    fn list_words(&self, filter: WordFilter) -> Result<Vec<Word>>;
    fn list_active(&self) -> Result<Vec<Word>> {
        self.list_words(WordFilter::Active)
    }
    fn list_learned(&self) -> Result<Vec<Word>> {
        self.list_words(WordFilter::Learned)
    }
    fn search_words(&self, query: &str, filter: WordFilter) -> Result<Vec<Word>>;
    fn count_words(&self, filter: WordFilter) -> Result<usize>;

    /// Replace a word's senses wholesale, in rank order. Enrichment owns this
    /// list, so a fresh lookup supersedes the previous one entirely.
    fn replace_senses(&mut self, word_id: WordId, senses: &[NewSense]) -> Result<()>;
    fn list_senses(&self, word_id: WordId) -> Result<Vec<Sense>>;
    fn get_sense(&self, id: SenseId) -> Result<Option<Sense>>;

    fn get_card(&self, word_id: WordId) -> Result<Option<ReviewCard>>;
    fn save_card(&mut self, card: &ReviewCard) -> Result<()>;

    fn record_review(
        &mut self,
        word_id: WordId,
        rating: Rating,
        reviewed_at: DateTime<Utc>,
    ) -> Result<Review>;
    fn list_reviews(&self, word_id: WordId) -> Result<Vec<Review>>;

    /// Active words joined with their cards, ready for the display selector.
    fn display_candidates(&self) -> Result<Vec<DisplayCandidate>>;
    fn touch_displayed(&mut self, id: WordId, now: DateTime<Utc>) -> Result<()>;

    /// The word currently shown in the panel, if the selector has run.
    fn current_word_id(&self) -> Result<Option<WordId>>;
    fn set_current_word_id(&mut self, id: Option<WordId>) -> Result<()>;
}
