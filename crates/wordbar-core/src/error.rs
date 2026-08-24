use thiserror::Error;

/// Errors produced by the Wordbar domain core and its storage implementations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("the term is empty")]
    EmptyTerm,

    #[error("the term is too long ({actual} characters, maximum {max})")]
    TermTooLong { actual: usize, max: usize },

    #[error("the term has too many words ({actual}, maximum {max})")]
    TooManyWords { actual: usize, max: usize },

    #[error("the term contains characters that are not allowed")]
    InvalidCharacters,

    #[error("\u{201c}{0}\u{201d} is already in your vocabulary")]
    DuplicateTerm(String),

    #[error("no word with id {0}")]
    UnknownWord(i64),

    #[error("the vocabulary is empty")]
    EmptyVocabulary,

    #[error("storage error: {0}")]
    Storage(String),

    #[error("dictionary lookup failed: {0}")]
    Dictionary(String),
}

pub type Result<T> = std::result::Result<T, Error>;
