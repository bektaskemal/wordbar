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

    /// Guess the format from the content, for a file whose name did not say.
    ///
    /// A hand-written word list is as likely to be `words.txt` as `words.csv`,
    /// and reading one as JSON fails outright -- so anything that does not
    /// open like JSON is treated as the forgiving format.
    pub fn sniff(data: &str) -> Self {
        match data.trim_start().chars().next() {
            Some('[') | Some('{') => Self::Json,
            _ => Self::Csv,
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

/// Import words, reporting what happened and which words were created.
///
/// The ids matter: an imported word usually arrives bare, so the caller has to
/// know what to look up afterwards.
pub fn import(
    engine: &mut Engine,
    data: &str,
    format: Format,
    now: DateTime<Utc>,
) -> Result<(ImportReport, Vec<wordbar_core::WordId>)> {
    let words = match format {
        Format::Json => parse_json(data)?,
        Format::Csv => parse_csv(data),
    };

    let mut report = ImportReport::default();
    let mut added = Vec::new();
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
            Ok(outcome) => {
                report.added += 1;
                // Only worth a lookup when the file did not supply one.
                if outcome.value.is_incomplete() {
                    added.push(outcome.value.id);
                }
            }
            Err(Error::DuplicateTerm(_)) => report.duplicates += 1,
            // A bad row must never abort the whole import.
            Err(_) => report.invalid += 1,
        }
    }
    Ok((report, added))
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
        // A bare list is very often `word pos` -- the shape a vocabulary
        // notebook or a study app exports. Only split when there is no other
        // column to disagree with, and only on a token that is unmistakably a
        // part of speech, so a two-word term stays intact.
        let (term, trailing_pos) = if fields.len() == 1 {
            split_trailing_part_of_speech(term)
        } else {
            (term, None)
        };

        out.push(PortableWord {
            term: term.to_string(),
            definition: field(&fields, 1),
            example: field(&fields, 2),
            part_of_speech: field(&fields, 3).or(trailing_pos),
        });
    }
    out
}

/// Split `haphazard adv` into `("haphazard", Some("adverb"))`.
///
/// Returns the line unchanged when the last token is not a part of speech, so
/// `at large` survives and `at large phrase` does not lose its head.
fn split_trailing_part_of_speech(term: &str) -> (&str, Option<String>) {
    let Some((head, last)) = term.rsplit_once(char::is_whitespace) else {
        return (term, None);
    };
    let head = head.trim_end();
    if head.is_empty() {
        return (term, None);
    }
    match expand_part_of_speech(last.trim().trim_end_matches('.')) {
        Some(pos) => (head, Some(pos.to_string())),
        None => (term, None),
    }
}

/// Map a part-of-speech abbreviation to the name dictionaries use, so an
/// imported tag can actually be matched against a looked-up sense.
fn expand_part_of_speech(token: &str) -> Option<&'static str> {
    let token = token.to_lowercase();
    Some(match token.as_str() {
        "n" | "noun" => "noun",
        "v" | "vb" | "verb" => "verb",
        "adj" | "a" | "adjective" => "adjective",
        "adv" | "adverb" => "adverb",
        "prep" | "preposition" => "preposition",
        "conj" | "conjunction" => "conjunction",
        "pron" | "pronoun" => "pronoun",
        "interj" | "int" | "interjection" => "interjection",
        "det" | "determiner" => "determiner",
        "abbr" | "abbreviation" => "abbreviation",
        "phrase" | "phr" | "idiom" => "phrase",
        _ => return None,
    })
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
    fn an_unnamed_format_is_sniffed_from_the_content() {
        assert_eq!(Format::sniff("nonchalant adj\n"), Format::Csv);
        assert_eq!(Format::sniff("laconic\n"), Format::Csv);
        assert_eq!(Format::sniff("  [{\"term\":\"a\"}]"), Format::Json);
        assert_eq!(Format::sniff("{}"), Format::Json);
        assert_eq!(Format::sniff(""), Format::Csv);
    }

    #[test]
    fn csv_accepts_term_only_rows() {
        let words = parse_csv("ephemeral\nubiquitous\n");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].term, "ephemeral");
        assert!(words[0].definition.is_none());
    }

    #[test]
    fn a_bare_list_may_tag_each_word_with_a_part_of_speech() {
        let words = parse_csv("nonchalant adj\nprofoundly adv\ndefy v\nanalogy n\n");
        let tagged: Vec<_> = words
            .iter()
            .map(|w| (w.term.as_str(), w.part_of_speech.as_deref()))
            .collect();
        assert_eq!(
            tagged,
            [
                ("nonchalant", Some("adjective")),
                ("profoundly", Some("adverb")),
                ("defy", Some("verb")),
                ("analogy", Some("noun")),
            ]
        );
        assert!(words[0].definition.is_none());
    }

    #[test]
    fn a_tagged_multi_word_term_keeps_its_head() {
        let words = parse_csv("at large phrase\n");
        assert_eq!(words[0].term, "at large");
        assert_eq!(words[0].part_of_speech.as_deref(), Some("phrase"));
    }

    #[test]
    fn an_untagged_multi_word_term_is_left_alone() {
        let words = parse_csv("ad hoc\nfait accompli\n");
        assert_eq!(words[0].term, "ad hoc");
        assert!(words[0].part_of_speech.is_none());
        assert_eq!(words[1].term, "fait accompli");
    }

    #[test]
    fn a_real_column_beats_a_trailing_tag() {
        let words = parse_csv("run v,to move quickly,,verb");
        assert_eq!(words[0].term, "run v");
        assert_eq!(words[0].part_of_speech.as_deref(), Some("verb"));
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
    fn import_reports_which_words_need_looking_up() {
        let store = crate::SqliteStore::open_in_memory().unwrap();
        let mut e = crate::Engine::new(Box::new(store));
        let (report, needing) = import(
            &mut e,
            "ephemeral\nlaconic,using few words\n",
            Format::Csv,
            chrono::Utc::now(),
        )
        .unwrap();

        assert_eq!(report.added, 2);
        assert_eq!(
            needing.len(),
            1,
            "only the word that arrived without a definition needs a lookup"
        );
        let word = e.get_word(needing[0]).unwrap();
        assert_eq!(word.term, "ephemeral");
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
