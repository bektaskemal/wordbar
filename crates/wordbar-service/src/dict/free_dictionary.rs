//! Provider backed by <https://dictionaryapi.dev>.
//!
//! Chosen because it needs no API key and no account, which keeps first-run
//! setup at zero steps. The trait in `wordbar-core` exists so this can be
//! swapped for a local dictionary later without touching the rest of the app.

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use wordbar_core::{
    rank_senses, DictionaryEntry, DictionaryProvider, EntrySense, Error, RankedSense, Result,
};

const ENDPOINT: &str = "https://api.dictionaryapi.dev/api/v2/entries/en";
const TIMEOUT: Duration = Duration::from_secs(8);

pub struct FreeDictionary {
    client: reqwest::Client,
    endpoint: String,
}

impl Default for FreeDictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl FreeDictionary {
    pub fn new() -> Self {
        Self::with_endpoint(
            std::env::var("WORDBAR_DICTIONARY_ENDPOINT").unwrap_or_else(|_| ENDPOINT.to_string()),
        )
    }

    pub fn with_endpoint(endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("wordbar/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client with static configuration");
        Self { client, endpoint }
    }
}

#[async_trait]
impl DictionaryProvider for FreeDictionary {
    async fn lookup(&self, term: &str) -> Result<Option<DictionaryEntry>> {
        let url = format!("{}/{}", self.endpoint, super::urlencode(term));
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

        let entries: Vec<ApiEntry> = response
            .json()
            .await
            .map_err(|e| Error::Dictionary(e.to_string()))?;

        Ok(convert(&entries))
    }
}

/// Flatten the API's nested shape into every sense it offered, ranked.
///
/// Nothing is discarded: a wrong pick should be a switch in the popup, not a
/// retype, so the alternatives have to survive the lookup.
fn convert(entries: &[ApiEntry]) -> Option<DictionaryEntry> {
    let entry = entries.first()?;

    let senses: Vec<RankedSense> = entry
        .meanings
        .iter()
        .flat_map(|meaning| {
            meaning.definitions.iter().filter_map(move |definition| {
                let text = definition.definition.trim();
                if text.is_empty() {
                    return None;
                }
                Some(RankedSense {
                    part_of_speech: Some(meaning.part_of_speech.clone()).filter(|p| !p.is_empty()),
                    definition: text.to_string(),
                    // An example only helps when it is a real sentence; the API
                    // sometimes returns fragments shorter than the term itself.
                    example: definition
                        .example
                        .clone()
                        .map(|e| e.trim().to_string())
                        .filter(|e| e.len() > 3),
                })
            })
        })
        .collect();

    let phonetic = entry
        .phonetic
        .clone()
        .or_else(|| entry.phonetics.iter().find_map(|p| p.text.clone()));

    let audio_url = entry
        .phonetics
        .iter()
        .find_map(|p| p.audio.clone().filter(|a| !a.trim().is_empty()));

    let result = DictionaryEntry {
        senses: rank_senses(senses)
            .into_iter()
            .map(|s| EntrySense {
                part_of_speech: s.part_of_speech,
                definition: s.definition,
                example: s.example,
            })
            .collect(),
        phonetic: phonetic
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty()),
        audio_url,
    };

    (!result.is_empty()).then_some(result)
}

#[derive(Debug, Deserialize)]
struct ApiEntry {
    #[serde(default)]
    phonetic: Option<String>,
    #[serde(default)]
    phonetics: Vec<ApiPhonetic>,
    #[serde(default)]
    meanings: Vec<ApiMeaning>,
}

#[derive(Debug, Deserialize)]
struct ApiPhonetic {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    audio: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiMeaning {
    #[serde(rename = "partOfSpeech")]
    part_of_speech: String,
    #[serde(default)]
    definitions: Vec<ApiDefinition>,
}

#[derive(Debug, Deserialize)]
struct ApiDefinition {
    definition: String,
    #[serde(default)]
    example: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{
        "word": "ephemeral",
        "phonetic": "/ɪˈfɛmərəl/",
        "phonetics": [{"text": "/ɪˈfɛmərəl/", "audio": "https://example.test/e.mp3"}],
        "meanings": [{
            "partOfSpeech": "adjective",
            "definitions": [{
                "definition": "Lasting for a very short time.",
                "example": "The opportunity was ephemeral."
            }]
        }]
    }]"#;

    #[test]
    fn converts_a_full_response() {
        let entries: Vec<ApiEntry> = serde_json::from_str(SAMPLE).unwrap();
        let entry = convert(&entries).expect("entry");
        let primary = entry.primary().expect("a sense");
        assert_eq!(primary.definition, "Lasting for a very short time.");
        assert_eq!(primary.part_of_speech.as_deref(), Some("adjective"));
        assert_eq!(
            primary.example.as_deref(),
            Some("The opportunity was ephemeral.")
        );
        assert_eq!(
            entry.audio_url.as_deref(),
            Some("https://example.test/e.mp3")
        );
        assert!(entry.phonetic.is_some());
    }

    #[test]
    fn an_empty_response_yields_nothing() {
        let entries: Vec<ApiEntry> = serde_json::from_str("[]").unwrap();
        assert!(convert(&entries).is_none());
    }

    #[test]
    fn missing_optional_fields_are_tolerated() {
        let json =
            r#"[{"meanings":[{"partOfSpeech":"noun","definitions":[{"definition":"A thing."}]}]}]"#;
        let entries: Vec<ApiEntry> = serde_json::from_str(json).unwrap();
        let entry = convert(&entries).expect("entry");
        let primary = entry.primary().unwrap();
        assert_eq!(primary.definition, "A thing.");
        assert!(primary.example.is_none());
        assert!(entry.phonetic.is_none());
    }

    /// The real ephemeral response: the API lists its single noun sense first.
    #[test]
    fn every_sense_survives_and_the_dominant_one_leads() {
        let json = r#"[{"meanings":[
            {"partOfSpeech":"noun","definitions":[{"definition":"Something short-lived."}]},
            {"partOfSpeech":"adjective","definitions":[
                {"definition":"Lasting a short time."},
                {"definition":"Existing for only one day."},
                {"definition":"Usually dry."}]}
        ]}]"#;
        let entries: Vec<ApiEntry> = serde_json::from_str(json).unwrap();
        let entry = convert(&entries).expect("entry");

        assert_eq!(entry.senses.len(), 4, "nothing is discarded");
        assert_eq!(
            entry.primary().unwrap().part_of_speech.as_deref(),
            Some("adjective"),
            "the noun sense must not lead just because it was listed first"
        );
    }
}
