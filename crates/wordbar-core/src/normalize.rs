//! Term normalization.
//!
//! Capture happens from arbitrary selected text, so the same word arrives in
//! many shapes: `\u{201c}Ephemeral\u{201d}`, `EPHEMERAL`, `ephemeral,`. All of
//! them must resolve to one vocabulary entry, while the user still sees the
//! capitalization they captured.

use unicode_normalization::UnicodeNormalization;

use crate::error::{Error, Result};

/// Maximum length of a term, in characters, after normalization.
pub const MAX_TERM_CHARS: usize = 80;
/// Maximum number of whitespace-separated words in a term.
pub const MAX_TERM_WORDS: usize = 5;

/// Characters stripped when they surround a captured term.
const STRIP_CHARS: &[char] = &[
    '"', '\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}', '\u{201c}', '\u{201d}', '\u{201e}',
    '\u{201f}', '\u{00ab}', '\u{00bb}', '\u{2039}', '\u{203a}', '`', '(', ')', '[', ']', '{', '}',
    '<', '>', '.', ',', ';', ':', '!', '?', '\u{2026}', '\u{2014}', '\u{2013}', '-', '*', '_', '#',
    '\u{00a1}', '\u{00bf}',
];

/// A term split into what the user sees and what we compare against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedTerm {
    /// Cleaned-up term with the original capitalization retained.
    pub display: String,
    /// Case-folded comparison key. Never shown to the user.
    pub key: String,
}

/// Clean a captured term and derive its comparison key.
///
/// Returns an error when nothing usable is left, so callers can reject a
/// capture before it reaches the database.
pub fn normalize_term(input: &str) -> Result<NormalizedTerm> {
    let display = clean_display(input);
    if display.is_empty() {
        return Err(Error::EmptyTerm);
    }

    let key = comparison_key(&display);
    if key.is_empty() {
        return Err(Error::EmptyTerm);
    }

    let term = NormalizedTerm { display, key };
    validate_term(&term)?;
    Ok(term)
}

/// Enforce the v1 shape limits: 1..=5 words, at most 80 characters, and at
/// least one letter so pure punctuation or digits never become vocabulary.
pub fn validate_term(term: &NormalizedTerm) -> Result<()> {
    let chars = term.display.chars().count();
    if chars > MAX_TERM_CHARS {
        return Err(Error::TermTooLong {
            actual: chars,
            max: MAX_TERM_CHARS,
        });
    }

    let words = term.display.split(' ').filter(|w| !w.is_empty()).count();
    if words == 0 {
        return Err(Error::EmptyTerm);
    }
    if words > MAX_TERM_WORDS {
        return Err(Error::TooManyWords {
            actual: words,
            max: MAX_TERM_WORDS,
        });
    }

    if !term.key.chars().any(char::is_alphabetic) {
        return Err(Error::InvalidCharacters);
    }
    if term.display.chars().any(|c| c.is_control()) {
        return Err(Error::InvalidCharacters);
    }

    Ok(())
}

/// True when a blob of text looks like it could be a vocabulary term.
///
/// Used by the capture path to decide between saving straight away and
/// opening a prefilled dialog; it never allocates a normalized term.
pub fn looks_like_term(input: &str) -> bool {
    normalize_term(input).is_ok()
}

/// Trim, collapse whitespace, unicode-normalize and strip wrapping punctuation.
fn clean_display(input: &str) -> String {
    // NFC keeps composed characters stable without folding away distinctions
    // the user may want to see (unlike NFKC, which we only use for the key).
    let composed: String = input.nfc().collect();

    let mut collapsed = String::with_capacity(composed.len());
    let mut pending_space = false;
    for ch in composed.chars() {
        if ch.is_whitespace() {
            pending_space = !collapsed.is_empty();
            continue;
        }
        if pending_space {
            collapsed.push(' ');
            pending_space = false;
        }
        collapsed.push(ch);
    }

    strip_surrounding(&collapsed).to_string()
}

/// Repeatedly peel punctuation off both ends, e.g. `("ephemeral"),` -> `ephemeral`.
fn strip_surrounding(s: &str) -> &str {
    let mut out = s;
    loop {
        let trimmed = out.trim_matches(|c: char| STRIP_CHARS.contains(&c) || c.is_whitespace());
        if trimmed.len() == out.len() {
            return trimmed;
        }
        out = trimmed;
    }
}

/// Aggressive fold used only for duplicate detection and lookup.
fn comparison_key(display: &str) -> String {
    display
        .nfkc()
        .map(|c| match c {
            // Typographic variants that users never intend as distinctions.
            '\u{2018}' | '\u{2019}' | '\u{02bc}' => '\'',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            other => other,
        })
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(input: &str) -> String {
        normalize_term(input).expect("should normalize").key
    }

    fn display(input: &str) -> String {
        normalize_term(input).expect("should normalize").display
    }

    #[test]
    fn quotes_case_and_punctuation_collapse_to_one_key() {
        let expected = "ephemeral";
        for input in [
            "ephemeral",
            "Ephemeral",
            "EPHEMERAL",
            "  ephemeral  ",
            "\u{201c}Ephemeral\u{201d}",
            "'ephemeral'",
            "(ephemeral)",
            "ephemeral.",
            "\u{201c}ephemeral,\u{201d}",
            "\u{00ab}ephemeral\u{00bb}",
        ] {
            assert_eq!(key(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn display_capitalization_is_retained() {
        assert_eq!(display("\u{201c}Ephemeral\u{201d}"), "Ephemeral");
        assert_eq!(display("NASA"), "NASA");
        assert_eq!(key("NASA"), "nasa");
    }

    #[test]
    fn repeated_whitespace_is_collapsed() {
        assert_eq!(display("ad   hoc"), "ad hoc");
        assert_eq!(display("by\tand\nlarge"), "by and large");
    }

    #[test]
    fn short_phrases_are_accepted() {
        for phrase in ["ad hoc", "by and large", "rule of thumb"] {
            assert_eq!(display(phrase), phrase);
        }
    }

    #[test]
    fn unicode_forms_normalize_together() {
        // "café" precomposed vs. decomposed.
        assert_eq!(key("caf\u{e9}"), key("cafe\u{301}"));
    }

    #[test]
    fn typographic_apostrophes_fold_to_ascii() {
        assert_eq!(key("don\u{2019}t"), key("don't"));
    }

    #[test]
    fn empty_and_punctuation_only_input_is_rejected() {
        for input in ["", "   ", "\u{201c}\u{201d}", "...", "-"] {
            assert!(matches!(
                normalize_term(input),
                Err(Error::EmptyTerm) | Err(Error::InvalidCharacters)
            ));
        }
    }

    #[test]
    fn digits_only_input_is_rejected() {
        assert!(matches!(
            normalize_term("12345"),
            Err(Error::InvalidCharacters)
        ));
    }

    #[test]
    fn too_many_words_is_rejected() {
        let err = normalize_term("one two three four five six").unwrap_err();
        assert!(matches!(err, Error::TooManyWords { actual: 6, max: 5 }));
    }

    #[test]
    fn too_long_is_rejected() {
        let long = "a".repeat(81);
        let err = normalize_term(&long).unwrap_err();
        assert!(matches!(
            err,
            Error::TermTooLong {
                actual: 81,
                max: 80
            }
        ));
    }

    #[test]
    fn five_words_and_eighty_chars_are_the_boundary() {
        assert!(normalize_term("one two three four five").is_ok());
        assert!(normalize_term(&"a".repeat(80)).is_ok());
    }

    #[test]
    fn internal_hyphens_survive() {
        assert_eq!(display("well-being"), "well-being");
        assert_eq!(key("well-being"), "well-being");
    }

    #[test]
    fn looks_like_term_rejects_prose() {
        assert!(looks_like_term("ephemeral"));
        assert!(!looks_like_term(
            "The effects were ephemeral, and nobody noticed them at all."
        ));
    }
}
