//! Pronunciation.
//!
//! Plays the dictionary's own recording when there is one, and falls back to
//! speech synthesis otherwise. Both happen here rather than in the GNOME
//! extension: the Shell is the compositor, and it has no business downloading
//! files or spawning media players.

use std::path::{Path, PathBuf};
use std::time::Duration;

use wordbar_core::{Error, Result};

/// Players tried in order. gst-play ships with GNOME; ffplay is a common
/// fallback. Both are asked to exit when the clip ends.
const PLAYERS: &[(&str, &[&str])] = &[
    ("gst-play-1.0", &["--no-interactive", "--quiet"]),
    ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
];

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10);
/// Recordings are a few tens of kilobytes; anything much larger is not one.
const MAX_AUDIO_BYTES: u64 = 4 * 1024 * 1024;

/// Where cached pronunciations live, alongside the database.
pub fn cache_dir() -> PathBuf {
    crate::paths::data_dir().join("audio")
}

/// Play a word's pronunciation, downloading and caching the recording first.
///
/// Falls back to synthesis when there is no recording, the download fails, or
/// no player is installed. Never returns an error for a missing player:
/// pronunciation is a convenience, not a feature to fail a request over.
pub async fn speak(term: &str, audio_url: Option<&str>) -> Result<()> {
    if let Some(url) = audio_url.filter(|u| !u.trim().is_empty()) {
        match cached_or_download(term, url).await {
            Ok(path) => {
                if play_file(&path).await {
                    return Ok(());
                }
                tracing::info!("no usable audio player; falling back to synthesis");
            }
            Err(err) => {
                tracing::info!(%err, "could not fetch the recording; falling back to synthesis");
            }
        }
    }
    synthesize(term).await
}

/// Return the cached file for this term, fetching it if absent.
async fn cached_or_download(term: &str, url: &str) -> Result<PathBuf> {
    let dir = cache_dir();
    let path = dir.join(cache_name(term, url));
    if path.is_file() {
        return Ok(path);
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::Storage(format!("cannot create {}: {e}", dir.display())))?;

    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(concat!("wordbar/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::Dictionary(e.to_string()))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Dictionary(e.to_string()))?;
    if !response.status().is_success() {
        return Err(Error::Dictionary(format!("HTTP {}", response.status())));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_AUDIO_BYTES {
            return Err(Error::Dictionary(format!(
                "recording too large ({length} bytes)"
            )));
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error::Dictionary(e.to_string()))?;
    if bytes.is_empty() {
        return Err(Error::Dictionary("empty recording".into()));
    }

    // Write via a temporary name so an interrupted download cannot be cached.
    let temporary = path.with_extension("part");
    std::fs::write(&temporary, &bytes)
        .map_err(|e| Error::Storage(format!("cannot write {}: {e}", temporary.display())))?;
    std::fs::rename(&temporary, &path)
        .map_err(|e| Error::Storage(format!("cannot finish {}: {e}", path.display())))?;

    Ok(path)
}

/// A stable, filesystem-safe name. The URL is hashed in so a changed
/// recording is refetched rather than served stale from the cache.
fn cache_name(term: &str, url: &str) -> String {
    let slug: String = term
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();

    // FNV-1a: no dependency, and collisions only cost a redundant download.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }

    let extension = url
        .rsplit('.')
        .next()
        .filter(|e| e.len() <= 4 && e.chars().all(char::is_alphanumeric))
        .unwrap_or("mp3");

    format!("{slug}-{hash:016x}.{extension}")
}

/// Try each known player; true once one has been started successfully.
async fn play_file(path: &Path) -> bool {
    for (binary, args) in PLAYERS {
        let mut command = tokio::process::Command::new(binary);
        command
            .args(*args)
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Ok(mut child) = command.spawn() {
            // Reaped so the service leaves no zombies behind.
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            tracing::debug!(player = binary, "playing recording");
            return true;
        }
    }
    false
}

/// Speech synthesis, for words with no recording. A missing speech-dispatcher
/// is not an error: the user simply hears nothing.
async fn synthesize(term: &str) -> Result<()> {
    let mut command = tokio::process::Command::new("spd-say");
    command
        .arg("--")
        .arg(term)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    match command.spawn() {
        Ok(mut child) => {
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(())
        }
        Err(err) => {
            tracing::info!(%err, "no speech synthesis available");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_names_are_safe_and_stable() {
        let a = cache_name("ad hoc", "https://example.test/a.mp3");
        assert_eq!(a, cache_name("ad hoc", "https://example.test/a.mp3"));
        assert!(!a.contains('/'), "{a} must not contain a path separator");
        assert!(a.starts_with("ad_hoc-"));
        assert!(a.ends_with(".mp3"));
    }

    #[test]
    fn a_changed_recording_gets_a_new_name() {
        let a = cache_name("ephemeral", "https://example.test/one.mp3");
        let b = cache_name("ephemeral", "https://example.test/two.mp3");
        assert_ne!(a, b, "a different URL must not reuse the cached file");
    }

    #[test]
    fn unicode_and_awkward_terms_are_sanitised() {
        let name = cache_name("caf\u{e9}/../x", "https://example.test/a.ogg");
        assert!(!name.contains(".."));
        assert!(!name.contains('/'));
        assert!(name.ends_with(".ogg"));
    }

    #[test]
    fn a_url_without_a_sensible_extension_defaults_to_mp3() {
        let name = cache_name("word", "https://example.test/audio?id=12345");
        assert!(name.ends_with(".mp3"), "{name}");
    }
}
