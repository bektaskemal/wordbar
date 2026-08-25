use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Primary key of a [`Word`].
pub type WordId = i64;

/// Primary key of a [`Review`].
pub type ReviewId = i64;

/// Lifecycle of a vocabulary entry.
///
/// Words are never silently destroyed: `Learned` takes an entry out of the
/// normal rotation while keeping its history intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WordStatus {
    Active,
    Learned,
}

impl WordStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Learned => "learned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "learned" => Some(Self::Learned),
            _ => None,
        }
    }
}

/// A vocabulary entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub id: WordId,
    /// The term exactly as the user captured it, capitalization preserved.
    pub term: String,
    /// Comparison key produced by [`crate::normalize_term`].
    pub normalized_term: String,

    pub definition: Option<String>,
    pub example: Option<String>,
    pub part_of_speech: Option<String>,
    pub phonetic: Option<String>,

    pub status: WordStatus,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Last time this word became the panel word. Display is *not* a review.
    pub last_displayed_at: Option<DateTime<Utc>>,
}

impl Word {
    /// True when enrichment has not produced a definition yet.
    pub fn is_incomplete(&self) -> bool {
        self.definition.as_deref().unwrap_or("").trim().is_empty()
    }
}

/// The spaced-repetition state belonging to a word.
///
/// This mirrors [`rs_fsrs::Card`] but is owned by Wordbar so the persistence
/// format does not follow upstream refactors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewCard {
    pub word_id: WordId,
    pub due: DateTime<Utc>,
    pub stability: f64,
    pub difficulty: f64,
    pub elapsed_days: i64,
    pub scheduled_days: i64,
    pub reps: i32,
    pub lapses: i32,
    /// `rs_fsrs::State` as its numeric discriminant (0=New .. 3=Relearning).
    pub state: i32,
    pub last_review: DateTime<Utc>,
}

impl ReviewCard {
    /// A brand-new card, due immediately.
    pub fn new(word_id: WordId, now: DateTime<Utc>) -> Self {
        Self {
            word_id,
            due: now,
            stability: 0.0,
            difficulty: 0.0,
            elapsed_days: 0,
            scheduled_days: 0,
            reps: 0,
            lapses: 0,
            state: rs_fsrs::State::New as i32,
            last_review: now,
        }
    }

    /// True while the card has never been rated.
    pub fn is_new(&self) -> bool {
        self.state == rs_fsrs::State::New as i32
    }

    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.due <= now
    }
}

/// What the user can say about a word in the popup.
///
/// Deliberately narrower than FSRS's four buttons: Wordbar is a glanceable
/// panel, not a flashcard app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    Again,
    Know,
}

impl Rating {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Again => "again",
            Self::Know => "know",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "again" => Some(Self::Again),
            "know" | "good" => Some(Self::Know),
            _ => None,
        }
    }
}

/// One recorded rating. History is kept from the very first review so undo,
/// debugging, migrations and future scheduler changes stay possible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub id: ReviewId,
    pub word_id: WordId,
    pub rating: Rating,
    pub reviewed_at: DateTime<Utc>,
}

/// One dictionary sense of a word.
///
/// Words keep every sense the dictionary offered; the one currently shown is
/// copied onto the [`Word`] itself so the panel and every client keep reading
/// a single definition without needing to know about senses at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sense {
    pub id: SenseId,
    pub word_id: WordId,
    /// Rank within the word, 0 first. Set by [`crate::rank_senses`].
    pub position: i64,
    pub part_of_speech: Option<String>,
    pub definition: String,
    pub example: Option<String>,
}

/// Primary key of a [`Sense`].
pub type SenseId = i64;

impl Sense {
    /// True when this sense is the one currently shown for its word.
    pub fn is_selected(&self, word: &Word) -> bool {
        word.definition.as_deref() == Some(self.definition.as_str())
    }

    /// "adjective · lasting for a very short time", for a chooser.
    pub fn summary(&self) -> String {
        match &self.part_of_speech {
            Some(pos) if !pos.is_empty() => format!("{pos} \u{b7} {}", self.definition),
            _ => self.definition.clone(),
        }
    }
}
