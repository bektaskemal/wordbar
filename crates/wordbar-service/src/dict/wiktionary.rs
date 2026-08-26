//! Provider backed by Wiktionary's REST API.
//!
//! Exists as a fallback for [`super::FreeDictionary`], which is a hobby service
//! that goes down for hours at a time -- and when it does, every word captured
//! meanwhile has no definition. This one is run by the Wikimedia Foundation off
//! the same infrastructure as Wikipedia, so it is unlikely to be down at the
//! same time.
//!
//! It is the fallback rather than the primary because its definitions are
//! written for a dictionary reader, not a learner: wordier, wrapped in markup,
//! and with no pronunciation recording. Better than nothing, worse than the
//! first choice.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use wordbar_core::{
    rank_senses, DictionaryEntry, DictionaryProvider, EntrySense, Error, RankedSense, Result,
};

const ENDPOINT: &str = "https://en.wiktionary.org/api/rest_v1/page/definition";
const TIMEOUT: Duration = Duration::from_secs(8);

pub struct Wiktionary {
    client: reqwest::Client,
    endpoint: String,
}

impl Default for Wiktionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Wiktionary {
    pub fn new() -> Self {
        Self::with_endpoint(
            std::env::var("WORDBAR_WIKTIONARY_ENDPOINT").unwrap_or_else(|_| ENDPOINT.to_string()),
        )
    }

    pub fn with_endpoint(endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!(
                "wordbar/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/bektaskemal/wordbar)"
            ))
            .build()
            .expect("reqwest client with static configuration");
        Self { client, endpoint }
    }
}

#[async_trait]
impl DictionaryProvider for Wiktionary {
    async fn lookup(&self, term: &str) -> Result<Option<DictionaryEntry>> {
        // Wiktionary titles use underscores for spaces, so a phrase needs that
        // rather than percent-encoding.
        let url = format!(
            "{}/{}",
            self.endpoint,
            super::urlencode(&term.replace(' ', "_"))
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Dictionary(e.to_string()))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error::Dictionary(format!("HTTP {}", response.status())));
        }

        let page: HashMap<String, Vec<Section>> = response
            .json()
            .await
            .map_err(|e| Error::Dictionary(e.to_string()))?;

        Ok(convert(&page))
    }
}

/// One part-of-speech section of a language's entry.
#[derive(Debug, Deserialize)]
struct Section {
    #[serde(rename = "partOfSpeech")]
    part_of_speech: Option<String>,
    #[serde(default)]
    definitions: Vec<Definition>,
}

#[derive(Debug, Deserialize)]
struct Definition {
    #[serde(default)]
    definition: String,
    #[serde(default)]
    examples: Vec<String>,
}

/// Flatten the English sections into ranked senses.
///
/// Only `en` is read: the same page carries every language that spells the word
/// this way, and a Dutch noun is not what the learner captured.
fn convert(page: &HashMap<String, Vec<Section>>) -> Option<DictionaryEntry> {
    let senses: Vec<RankedSense> = page
        .get("en")?
        .iter()
        .flat_map(|section| {
            let part_of_speech = section
                .part_of_speech
                .as_deref()
                .map(str::to_lowercase)
                .filter(|p| !p.is_empty());
            section.definitions.iter().filter_map(move |definition| {
                let text = strip_markup(&definition.definition);
                // Sections routinely carry an empty lead entry.
                if text.is_empty() {
                    return None;
                }
                Some(RankedSense {
                    part_of_speech: part_of_speech.clone(),
                    definition: text,
                    example: definition
                        .examples
                        .iter()
                        .map(|e| strip_markup(e))
                        .find(|e| e.len() > 3),
                })
            })
        })
        .collect();

    let result = DictionaryEntry {
        senses: rank_senses(senses)
            .into_iter()
            .map(|s| EntrySense {
                part_of_speech: s.part_of_speech,
                definition: s.definition,
                example: s.example,
            })
            .collect(),
        // The REST API carries neither, which is the other reason this is the
        // fallback. Speech synthesis covers the gap.
        phonetic: None,
        audio_url: None,
    };

    (!result.is_empty()).then_some(result)
}

/// Turn a definition's HTML into the plain sentence underneath it.
///
/// Wiktionary returns wikitext rendered to HTML: links, bold, and transclusion
/// spans for usage labels. Nothing here needs a parser -- the markup is
/// well-formed and only ever wraps the text we want.
fn strip_markup(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut depth = 0usize;
    for ch in html.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(text: &str) -> String {
    // The handful the API actually emits. An unknown entity is left alone
    // rather than mangled.
    const NAMED: [(&str, &str); 6] = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&nbsp;", " "),
    ];
    let mut out = text.to_string();
    for (from, to) in NAMED {
        if out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_reduced_to_the_sentence() {
        let html = "Lack of <a rel=\"mw:WikiLink\" href=\"/wiki/emotion\">emotion</a> \
                    or motivation.";
        assert_eq!(strip_markup(html), "Lack of emotion or motivation.");
    }

    #[test]
    fn a_usage_label_span_leaves_no_gap() {
        let html = "<span class=\"usage-label-sense\" typeof=\"mw:Transclusion\"></span> \
                    To have a <b>frown</b> on one's face.";
        assert_eq!(strip_markup(html), "To have a frown on one's face.");
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(strip_markup("salt &amp; pepper"), "salt & pepper");
        assert_eq!(strip_markup("it&#39;s here"), "it's here");
    }

    #[test]
    fn an_empty_definition_reduces_to_nothing() {
        assert_eq!(strip_markup("<span></span>"), "");
        assert_eq!(strip_markup(""), "");
    }

    fn page(sections: &[(&str, &[&str])]) -> HashMap<String, Vec<Section>> {
        let mut map = HashMap::new();
        map.insert(
            "en".to_string(),
            sections
                .iter()
                .map(|(pos, defs)| Section {
                    part_of_speech: Some((*pos).to_string()),
                    definitions: defs
                        .iter()
                        .map(|d| Definition {
                            definition: (*d).to_string(),
                            examples: Vec::new(),
                        })
                        .collect(),
                })
                .collect(),
        );
        map
    }

    #[test]
    fn the_part_of_speech_is_lowercased_to_match_the_other_provider() {
        let entry = convert(&page(&[("Noun", &["A thing."])])).unwrap();
        assert_eq!(entry.senses[0].part_of_speech.as_deref(), Some("noun"));
    }

    #[test]
    fn empty_lead_definitions_are_dropped() {
        let entry = convert(&page(&[("Noun", &["", "A wrinkling of the forehead."])])).unwrap();
        assert_eq!(entry.senses.len(), 1);
        assert_eq!(entry.senses[0].definition, "A wrinkling of the forehead.");
    }

    #[test]
    fn a_page_with_no_english_section_is_not_an_entry() {
        let mut map = HashMap::new();
        map.insert("nl".to_string(), Vec::new());
        assert!(convert(&map).is_none());
    }

    #[test]
    fn a_page_with_nothing_usable_is_not_an_entry() {
        assert!(convert(&page(&[("Noun", &["", "<span></span>"])])).is_none());
    }
}
