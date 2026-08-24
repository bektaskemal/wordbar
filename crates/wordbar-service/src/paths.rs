//! XDG locations. One local database, no configuration files.

use std::path::PathBuf;

/// `$XDG_DATA_HOME/wordbar/wordbar.db`, falling back to `~/.local/share`.
pub fn database_path() -> PathBuf {
    data_dir().join("wordbar.db")
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WORDBAR_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/share"));
    base.join("wordbar")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}
