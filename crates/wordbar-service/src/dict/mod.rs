//! Dictionary enrichment.
//!
//! Enrichment runs after a word is already stored, so it is always allowed to
//! fail. `WORDBAR_DICTIONARY=off` disables it entirely for offline use or
//! tests.

pub mod free_dictionary;

use std::sync::Arc;

use wordbar_core::dictionary::NullDictionary;
use wordbar_core::DictionaryProvider;

pub use free_dictionary::FreeDictionary;

/// Build the provider configured for this run.
pub fn default_provider() -> Arc<dyn DictionaryProvider> {
    match std::env::var("WORDBAR_DICTIONARY").as_deref() {
        Ok("off") | Ok("none") => {
            tracing::info!("dictionary enrichment disabled by WORDBAR_DICTIONARY");
            Arc::new(NullDictionary)
        }
        _ => Arc::new(FreeDictionary::new()),
    }
}
