//! Provider backed by <https://dictionaryapi.dev>.
//!
//! Chosen because it needs no API key and no account, which keeps first-run
//! setup at zero steps. The trait in `wordbar-core` exists so this can be
//! swapped for a local dictionary later without touching the rest of the app.

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use wordbar_core::{DictionaryEntry, DictionaryProvider, Error, Result};

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
        let url = format!("{}/{}", self.endpoint, urlencode(term));
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

/// Flatten the API's nested shape into the single best definition we show.
fn convert(entries: &[ApiEntry]) -> Option<DictionaryEntry> {
    let entry = entries.first()?;

    let usable = |m: &&ApiMeaning| {
        m.definitions
            .iter()
            .any(|d| !d.definition.trim().is_empty())
    };

    // A sense that ships an example makes a much better card, so prefer one
    // even when the source lists it second; otherwise take the first sense
    // that has a definition at all.
    let meaning = entry
        .meanings
        .iter()
        .find(|m| usable(m) && m.definitions.iter().any(|d| d.example.is_some()))
        .or_else(|| entry.meanings.iter().find(usable));

    let definition = meaning.and_then(|m| {
        m.definitions
            .iter()
            .find(|d| d.example.is_some())
            .or_else(|| m.definitions.first())
    });

    let phonetic = entry
        .phonetic
        .clone()
        .or_else(|| entry.phonetics.iter().find_map(|p| p.text.clone()));

    let audio_url = entry
        .phonetics
        .iter()
        .find_map(|p| p.audio.clone().filter(|a| !a.trim().is_empty()));

    // An example only helps when it is a real sentence; the API sometimes
    // returns fragments shorter than the term itself.
    let example = definition
        .and_then(|d| d.example.clone())
        .map(|e| e.trim().to_string())
        .filter(|e| e.len() > 3);

    let result = DictionaryEntry {
        definition: definition.map(|d| d.definition.trim().to_string()),
        example,
        part_of_speech: meaning.map(|m| m.part_of_speech.clone()),
        phonetic: phonetic
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty()),
        audio_url,
    };

    (!result.is_empty()).then_some(result)
}

fn urlencode(term: &str) -> String {
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
        assert_eq!(
            entry.definition.as_deref(),
            Some("Lasting for a very short time.")
        );
        assert_eq!(entry.part_of_speech.as_deref(), Some("adjective"));
        assert_eq!(
            entry.example.as_deref(),
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
        assert_eq!(entry.definition.as_deref(), Some("A thing."));
        assert!(entry.example.is_none());
        assert!(entry.phonetic.is_none());
    }

    #[test]
    fn a_sense_with_an_example_wins_over_an_earlier_bare_one() {
        let json = r#"[{"meanings":[
            {"partOfSpeech":"noun","definitions":[{"definition":"A short-lived thing."}]},
            {"partOfSpeech":"adjective","definitions":[
                {"definition":"Lasting a short time.","example":"An ephemeral fashion."}]}
        ]}]"#;
        let entries: Vec<ApiEntry> = serde_json::from_str(json).unwrap();
        let entry = convert(&entries).expect("entry");
        assert_eq!(entry.part_of_speech.as_deref(), Some("adjective"));
        assert_eq!(entry.example.as_deref(), Some("An ephemeral fashion."));
    }

    #[test]
    fn without_examples_the_first_sense_is_used() {
        let json = r#"[{"meanings":[
            {"partOfSpeech":"noun","definitions":[{"definition":"A short-lived thing."}]},
            {"partOfSpeech":"adjective","definitions":[{"definition":"Lasting a short time."}]}
        ]}]"#;
        let entries: Vec<ApiEntry> = serde_json::from_str(json).unwrap();
        let entry = convert(&entries).expect("entry");
        assert_eq!(entry.part_of_speech.as_deref(), Some("noun"));
        assert!(entry.example.is_none());
    }

    #[test]
    fn terms_with_spaces_are_percent_encoded() {
        assert_eq!(urlencode("ad hoc"), "ad%20hoc");
        assert_eq!(urlencode("caf\u{e9}"), "caf%C3%A9");
    }
}
