//! Dictionary enrichment.
//!
//! Enrichment runs after a word is already stored, so it is always allowed to
//! fail. `WORDBAR_DICTIONARY=off` disables it entirely for offline use or
//! tests.

pub mod free_dictionary;
pub mod wiktionary;

use std::sync::Arc;

use async_trait::async_trait;
use wordbar_core::dictionary::NullDictionary;
use wordbar_core::{DictionaryEntry, DictionaryProvider, Result};

pub use free_dictionary::FreeDictionary;
pub use wiktionary::Wiktionary;

/// Build the provider configured for this run.
///
/// Two sources, tried in order. The first is the better read; the second is
/// far more likely to be up. `WORDBAR_DICTIONARY=primary` pins the first alone,
/// which is what the retry test wants.
pub fn default_provider() -> Arc<dyn DictionaryProvider> {
    match std::env::var("WORDBAR_DICTIONARY").as_deref() {
        Ok("off") | Ok("none") => {
            tracing::info!("dictionary enrichment disabled by WORDBAR_DICTIONARY");
            Arc::new(NullDictionary)
        }
        Ok("primary") => Arc::new(FreeDictionary::new()),
        _ => Arc::new(Chain::new(vec![
            Box::new(FreeDictionary::new()),
            Box::new(Wiktionary::new()),
        ])),
    }
}

/// Ask each provider in turn until one has an answer.
///
/// The distinction between "nobody has this word" and "nobody could be reached"
/// has to survive, because the enrichment worker retries the second and not the
/// first. So a provider that errors makes the whole chain error, even if a
/// later one said "no entry" -- otherwise an outage at the head of the chain
/// would permanently mark words as absent.
pub struct Chain {
    providers: Vec<Box<dyn DictionaryProvider>>,
}

impl Chain {
    pub fn new(providers: Vec<Box<dyn DictionaryProvider>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl DictionaryProvider for Chain {
    async fn lookup(&self, term: &str) -> Result<Option<DictionaryEntry>> {
        let mut last_error = None;

        for provider in &self.providers {
            match provider.lookup(term).await {
                Ok(Some(entry)) => return Ok(Some(entry)),
                Ok(None) => {}
                Err(err) => {
                    tracing::debug!(%term, %err, "provider failed; trying the next");
                    last_error = Some(err);
                }
            }
        }

        match last_error {
            Some(err) => Err(err),
            None => Ok(None),
        }
    }
}

/// Percent-encode a term for a path segment. Shared by the providers.
pub(crate) fn urlencode(term: &str) -> String {
    term.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordbar_core::Error;

    #[test]
    fn terms_with_spaces_are_percent_encoded() {
        assert_eq!(urlencode("ad hoc"), "ad%20hoc");
        assert_eq!(urlencode("caf\u{e9}"), "caf%C3%A9");
    }

    /// A provider with a scripted answer.
    struct Stub(Result<Option<DictionaryEntry>>);

    #[async_trait]
    impl DictionaryProvider for Stub {
        async fn lookup(&self, _term: &str) -> Result<Option<DictionaryEntry>> {
            match &self.0 {
                Ok(entry) => Ok(entry.clone()),
                Err(err) => Err(Error::Dictionary(err.to_string())),
            }
        }
    }

    fn entry(definition: &str) -> DictionaryEntry {
        DictionaryEntry {
            senses: vec![wordbar_core::EntrySense {
                part_of_speech: Some("noun".into()),
                definition: definition.to_string(),
                example: None,
            }],
            ..DictionaryEntry::default()
        }
    }

    fn down() -> Stub {
        Stub(Err(Error::Dictionary("HTTP 522".into())))
    }

    fn absent() -> Stub {
        Stub(Ok(None))
    }

    fn found(definition: &str) -> Stub {
        Stub(Ok(Some(entry(definition))))
    }

    #[tokio::test]
    async fn the_first_answer_wins() {
        let chain = Chain::new(vec![Box::new(found("first")), Box::new(found("second"))]);
        let got = chain.lookup("x").await.unwrap().unwrap();
        assert_eq!(got.senses[0].definition, "first");
    }

    #[tokio::test]
    async fn a_failing_provider_falls_through_to_the_next() {
        let chain = Chain::new(vec![Box::new(down()), Box::new(found("fallback"))]);
        let got = chain.lookup("x").await.unwrap().unwrap();
        assert_eq!(got.senses[0].definition, "fallback");
    }

    #[tokio::test]
    async fn a_provider_without_the_word_falls_through_too() {
        let chain = Chain::new(vec![Box::new(absent()), Box::new(found("fallback"))]);
        assert!(chain.lookup("x").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn everyone_answering_no_entry_is_a_real_absence() {
        let chain = Chain::new(vec![Box::new(absent()), Box::new(absent())]);
        assert!(chain.lookup("x").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn an_outage_anywhere_is_an_error_not_an_absence() {
        // Otherwise the worker would mark the word absent and stop retrying,
        // and it would never be looked up again once the outage ended.
        let chain = Chain::new(vec![Box::new(down()), Box::new(absent())]);
        assert!(chain.lookup("x").await.is_err());
        let chain = Chain::new(vec![Box::new(absent()), Box::new(down())]);
        assert!(chain.lookup("x").await.is_err());
    }
}
