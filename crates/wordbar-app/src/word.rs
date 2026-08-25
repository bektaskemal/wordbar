//! The app's view of a word, decoded from the service's `a{sv}`.

use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

pub type WordDict = HashMap<String, OwnedValue>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Word {
    pub id: i64,
    pub term: String,
    pub definition: String,
    pub example: String,
    pub part_of_speech: String,
    pub phonetic: String,
    pub learned: bool,
}

impl Word {
    pub fn from_dict(dict: &WordDict) -> Option<Self> {
        if dict.is_empty() {
            return None;
        }
        let term = text(dict, "term");
        if term.is_empty() {
            return None;
        }
        Some(Self {
            id: dict
                .get("id")
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or_default(),
            term,
            definition: text(dict, "definition"),
            example: text(dict, "example"),
            part_of_speech: text(dict, "part_of_speech"),
            phonetic: text(dict, "phonetic"),
            learned: text(dict, "status") == "learned",
        })
    }

    /// What the list row shows under the term.
    pub fn subtitle(&self) -> String {
        if self.definition.is_empty() {
            "Definition unavailable".to_string()
        } else {
            self.definition.clone()
        }
    }
}

fn text(dict: &WordDict, key: &str) -> String {
    dict.get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or_default()
        .to_string()
}

/// One alternative sense of a word, for the editor's chooser.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sense {
    pub id: i64,
    pub summary: String,
    pub part_of_speech: String,
    pub definition: String,
    pub example: String,
    pub selected: bool,
}

impl Sense {
    pub fn from_dict(dict: &WordDict) -> Option<Self> {
        let definition = text(dict, "definition");
        if definition.is_empty() {
            return None;
        }
        Some(Self {
            id: dict
                .get("id")
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or_default(),
            summary: text(dict, "summary"),
            part_of_speech: text(dict, "part_of_speech"),
            definition,
            example: text(dict, "example"),
            selected: dict
                .get("selected")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false),
        })
    }
}
