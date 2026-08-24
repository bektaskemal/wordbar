//! Client-side view of a word.
//!
//! The service sends `a{sv}` so fields can be added without breaking clients;
//! this turns that back into something typed for printing.

use serde::Serialize;
use zbus::zvariant::OwnedValue;

use crate::proxy::WordDict;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Word {
    pub id: i64,
    pub term: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_of_speech: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,
    pub status: String,
}

impl Word {
    /// `None` for the empty dictionary the service returns when there is no
    /// word, rather than an error: "nothing to show" is a normal state.
    pub fn from_dict(dict: &WordDict) -> Option<Self> {
        if dict.is_empty() {
            return None;
        }
        Some(Self {
            id: int(dict, "id"),
            term: text(dict, "term")?,
            definition: text(dict, "definition"),
            example: text(dict, "example"),
            part_of_speech: text(dict, "part_of_speech"),
            phonetic: text(dict, "phonetic"),
            status: text(dict, "status").unwrap_or_else(|| "active".into()),
        })
    }

    /// One-line human output: the term, then the definition when we have one.
    pub fn plain(&self) -> String {
        match &self.definition {
            Some(def) => format!("{}\n{}", self.term, def),
            None => format!("{}\nDefinition unavailable", self.term),
        }
    }
}

fn text(dict: &WordDict, key: &str) -> Option<String> {
    let value: &OwnedValue = dict.get(key)?;
    <&str>::try_from(value)
        .ok()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn int(dict: &WordDict, key: &str) -> i64 {
    dict.get(key)
        .and_then(|v| i64::try_from(v).ok())
        .unwrap_or_default()
}
