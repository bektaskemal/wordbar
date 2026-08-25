//! Import and export of *portable vocabulary*.
//!
//! Deliberately excludes scheduler internals: an export is "my words", not "my
//! Wordbar installation". Full backup/restore (cards, review history,
//! settings) is a separate, later feature — the schema keeps it possible.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wordbar_core::{Error, NewWord, Result, WordFilter};

use crate::engine::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Csv,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "json" => Some(Self::Json),
            "csv" => Some(Self::Csv),
            _ => None,
        }
    }

    /// Guess from a filename, defaulting to JSON.
    pub fn from_path(path: &str) -> Self {
        if path.to_lowercase().ends_with(".csv") {
            Self::Csv
        } else {
            Self::Json
        }
    }
}

/// One exported entry. Field names are stable and match the CSV header.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortableWord {
    pub term: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_of_speech: Option<String>,
}

/// What an import did. Nothing is overwritten; duplicates are simply skipped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub added: usize,
    pub duplicates: usize,
    pub invalid: usize,
}

pub fn export(engine: &Engine, filter: WordFilter, format: Format) -> Result<String> {
    let words: Vec<PortableWord> = engine
        .list_words(filter, "")?
        .into_iter()
        .map(|w| PortableWord {
            term: w.term,
            definition: w.definition,
            example: w.example,
            part_of_speech: w.part_of_speech,
        })
        .collect();

    match format {
        Format::Json => {
            serde_json::to_string_pretty(&words).map_err(|e| Error::Storage(e.to_string()))
        }
        Format::Csv => Ok(to_csv(&words)),
    }
}

pub fn import(
    engine: &mut Engine,
    data: &str,
    format: Format,
    now: DateTime<Utc>,
) -> Result<ImportReport> {
    let words = match format {
        Format::Json => parse_json(data)?,
        Format::Csv => parse_csv(data),
    };

    let mut report = ImportReport::default();
    for word in words {
        let new = NewWord {
            term: word.term,
            definition: word.definition,
            example: word.example,
            part_of_speech: word.part_of_speech,
            phonetic: None,
            audio_url: None,
        };
        match engine.add_word_full(new, now) {
            Ok(_) => report.added += 1,
            Err(Error::DuplicateTerm(_)) => report.duplicates += 1,
            // A bad row must never abort the whole import.
            Err(_) => report.invalid += 1,
        }
    }
    Ok(report)
}

fn parse_json(data: &str) -> Result<Vec<PortableWord>> {
    serde_json::from_str(data).map_err(|e| Error::Storage(format!("invalid JSON: {e}")))
}

/// Accept progressively richer CSV: `term`, `term,definition`,
/// `term,definition,example,part_of_speech`. A header row is optional.
fn parse_csv(data: &str) -> Vec<PortableWord> {
    let mut out = Vec::new();
    for (index, line) in data.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_csv_line(line);
        let term = fields.first().map(String::as_str).unwrap_or("").trim();
        if term.is_empty() {
            continue;
        }
        if index == 0 && term.eq_ignore_ascii_case("term") {
            continue; // header
        }
        out.push(PortableWord {
            term: term.to_string(),
            definition: field(&fields, 1),
            example: field(&fields, 2),
            part_of_speech: field(&fields, 3),
        });
    }
    out
}

fn field(fields: &[String], index: usize) -> Option<String> {
    fields
        .get(index)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Minimal RFC 4180 reader: quoted fields with `""` escapes.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => fields.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    fields.push(current);
    fields
}

fn to_csv(words: &[PortableWord]) -> String {
    let mut out = String::from("term,definition,example,part_of_speech\n");
    for word in words {
        let row = [
            Some(word.term.as_str()),
            word.definition.as_deref(),
            word.example.as_deref(),
            word.part_of_speech.as_deref(),
        ]
        .map(|v| escape_csv(v.unwrap_or("")))
        .join(",");
        out.push_str(&row);
        out.push('\n');
    }
    out
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_accepts_term_only_rows() {
        let words = parse_csv("ephemeral\nubiquitous\n");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].term, "ephemeral");
        assert!(words[0].definition.is_none());
    }

    #[test]
    fn csv_accepts_term_and_definition() {
        let words = parse_csv("ephemeral,lasting for a very short time");
        assert_eq!(
            words[0].definition.as_deref(),
            Some("lasting for a very short time")
        );
    }

    #[test]
    fn csv_accepts_the_full_row_and_skips_the_header() {
        let data = "term,definition,example,part_of_speech\n\
                    ephemeral,\"lasting, briefly\",\"It was ephemeral.\",adjective\n";
        let words = parse_csv(data);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].definition.as_deref(), Some("lasting, briefly"));
        assert_eq!(words[0].part_of_speech.as_deref(), Some("adjective"));
    }

    #[test]
    fn csv_roundtrips_values_containing_commas_and_quotes() {
        let words = vec![PortableWord {
            term: "ad hoc".into(),
            definition: Some("for this, only this".into()),
            example: Some("She said \"ad hoc\".".into()),
            part_of_speech: Some("adjective".into()),
        }];
        let parsed = parse_csv(&to_csv(&words));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].definition, words[0].definition);
        assert_eq!(parsed[0].example, words[0].example);
    }

    #[test]
    fn json_export_has_no_scheduler_internals() {
        let words = vec![PortableWord {
            term: "ephemeral".into(),
            definition: Some("lasting for a very short time".into()),
            ..Default::default()
        }];
        let json = serde_json::to_string(&words).unwrap();
        assert!(!json.contains("stability"));
        assert!(!json.contains("difficulty"));
        assert!(!json.contains("due"));
        assert!(!json.contains("example"), "empty fields are omitted");
    }
}
