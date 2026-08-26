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

    /// The best-ranked sense of a given part of speech, falling back to
    /// [`Self::primary`].
    ///
    /// An imported list often carries a part-of-speech tag precisely because
    /// the word was met in that role: `haphazard adv` should not come back as
    /// an adjective just because that sense is more common overall.
    pub fn preferred(&self, part_of_speech: Option<&str>) -> Option<&EntrySense> {
        let wanted = part_of_speech.map(str::trim).filter(|p| !p.is_empty());
        wanted
            .and_then(|wanted| {
                self.senses.iter().find(|s| {
                    s.part_of_speech
                        .as_deref()
                        .is_some_and(|p| p.eq_ignore_ascii_case(wanted))
                })
            })
            .or_else(|| self.primary())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(senses: &[(&str, &str)]) -> DictionaryEntry {
        DictionaryEntry {
            senses: senses
                .iter()
                .map(|(pos, def)| EntrySense {
                    part_of_speech: Some((*pos).to_string()),
                    definition: (*def).to_string(),
                    example: None,
                })
                .collect(),
            ..DictionaryEntry::default()
        }
    }

    #[test]
    fn a_requested_part_of_speech_wins_over_the_ranking() {
        let e = entry(&[("adjective", "not planned"), ("adverb", "at random")]);
        assert_eq!(e.preferred(Some("adverb")).unwrap().definition, "at random");
    }

    #[test]
    fn an_absent_or_unmatched_part_of_speech_falls_back_to_the_primary() {
        let e = entry(&[("adjective", "not planned"), ("adverb", "at random")]);
        assert_eq!(e.preferred(None).unwrap().definition, "not planned");
        assert_eq!(e.preferred(Some("noun")).unwrap().definition, "not planned");
        assert_eq!(e.preferred(Some("  ")).unwrap().definition, "not planned");
    }

    #[test]
    fn matching_a_part_of_speech_ignores_case() {
        let e = entry(&[("adjective", "not planned"), ("Adverb", "at random")]);
        assert_eq!(e.preferred(Some("ADVERB")).unwrap().definition, "at random");
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
