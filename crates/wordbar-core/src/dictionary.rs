//! Dictionary enrichment abstraction.
//!
//! Enrichment is always best-effort and always happens *after* the word is
//! saved, so a failed lookup can never fail a capture.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// What a provider can tell us about a term. Every field is optional because
/// sources differ in completeness.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub definition: Option<String>,
    pub example: Option<String>,
    pub part_of_speech: Option<String>,
    pub phonetic: Option<String>,
    /// URL of a pronunciation clip, when the source offers one.
    pub audio_url: Option<String>,
}

impl DictionaryEntry {
    pub fn is_empty(&self) -> bool {
        self.definition.is_none()
            && self.example.is_none()
            && self.part_of_speech.is_none()
            && self.phonetic.is_none()
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
