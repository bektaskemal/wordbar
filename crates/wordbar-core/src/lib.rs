//! Wordbar domain core.
//!
//! This crate owns everything that is *not* I/O: the vocabulary model, term
//! normalization, the spaced-repetition mapping and the panel display policy.
//! Storage and dictionary access are expressed as traits so the core stays
//! testable without SQLite, D-Bus or the network.

pub mod dictionary;
pub mod error;
pub mod model;
pub mod normalize;
pub mod scheduler;
pub mod select;
pub mod senses;
pub mod storage;

pub use dictionary::{DictionaryEntry, DictionaryProvider, EntrySense};
pub use error::{Error, Result};
pub use model::{Rating, Review, ReviewCard, ReviewId, Sense, SenseId, Word, WordId, WordStatus};
pub use normalize::{normalize_term, validate_term, NormalizedTerm};
pub use scheduler::{FsrsScheduler, Scheduler};
pub use select::{select_display_word, DisplayCandidate, SelectionPolicy};
pub use senses::{rank_senses, RankedSense};
pub use storage::{NewSense, NewWord, Storage, WordFilter, WordPatch};
