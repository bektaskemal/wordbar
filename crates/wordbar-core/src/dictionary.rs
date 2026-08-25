//! Dictionary enrichment abstraction.
//!
//! Enrichment is always best-effort and always happens *after* the word is
//! saved, so a failed lookup can never fail a capture.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// What a provider can tell us about a term.
///
/// Every sense is carried, ranked by [`crate::rank_senses`], because a single
/// stored definition picks the wrong one often enough to matter -- and a wrong
/// pick should be a switch, not a retype.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub senses: Vec<EntrySense>,
    pub phonetic: Option<String>,
    /// URL of a pronunciation recording, when the source offers one.
    pub audio_url: Option<String>,
}

/// One sense of a looked-up term.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntrySense {
    pub part_of_speech: Option<String>,
    pub definition: String,
    pub example: Option<String>,
}

impl DictionaryEntry {
    pub fn is_empty(&self) -> bool {
        self.senses.is_empty() && self.phonetic.is_none() && self.audio_url.is_none()
    }

    /// The sense to show, i.e. the best-ranked one.
    pub fn primary(&self) -> Option<&EntrySense> {
        self.senses.first()
    }
}

#[async_trait]
pub trait DictionaryProvider: Send + Sync {
    /// Look up a term. `Ok(None)` means "not found", `Err` means the lookup
    /// itself failed (offline, timeout, bad response).
    async fn lookup(&self, term: &str) -> Result<Option<DictionaryEntry>>;
}

/// A provider that never finds anything. Used in tests and when enrichment is
/// disabled, so the rest of the system does not need a special case.
#[derive(Debug, Clone, Default)]
pub struct NullDictionary;

#[async_trait]
impl DictionaryProvider for NullDictionary {
    async fn lookup(&self, _term: &str) -> Result<Option<DictionaryEntry>> {
        Ok(None)
    }
}
