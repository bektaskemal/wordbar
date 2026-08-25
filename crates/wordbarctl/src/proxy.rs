//! Generated client for `org.wordbar.Wordbar`.
//!
//! `wordbarctl` never opens the database. Everything goes over D-Bus, which is
//! also what makes this binary the primary debugging tool for the backend.

use std::collections::HashMap;

use zbus::proxy;
use zbus::zvariant::OwnedValue;

pub type WordDict = HashMap<String, OwnedValue>;

#[proxy(
    interface = "org.wordbar.Wordbar",
    default_service = "org.wordbar.Wordbar",
    default_path = "/org/wordbar/Wordbar"
)]
pub trait Wordbar {
    fn add_word(&self, text: &str) -> zbus::Result<WordDict>;
    fn find_word(&self, term: &str) -> zbus::Result<WordDict>;

    fn get_current_word(&self) -> zbus::Result<WordDict>;
    fn advance_display(&self) -> zbus::Result<WordDict>;
    fn get_word_details(&self, id: i64) -> zbus::Result<WordDict>;

    fn rate_again(&self, id: i64) -> zbus::Result<WordDict>;
    fn rate_know(&self, id: i64) -> zbus::Result<WordDict>;
    fn mark_learned(&self, id: i64) -> zbus::Result<WordDict>;
    fn restore_word(&self, id: i64) -> zbus::Result<WordDict>;
    fn delete_word(&self, id: i64) -> zbus::Result<()>;
    fn refresh_word(&self, id: i64) -> zbus::Result<u32>;
    fn update_word(&self, id: i64, fields: HashMap<&str, &str>) -> zbus::Result<WordDict>;

    fn list_words(&self, filter: &str, query: &str) -> zbus::Result<Vec<WordDict>>;
    fn get_stats(&self) -> zbus::Result<(u32, u32)>;

    fn export_vocabulary(&self, filter: &str, format: &str) -> zbus::Result<String>;
    fn import_vocabulary(&self, data: &str, format: &str) -> zbus::Result<(u32, u32, u32)>;

    fn open_vocabulary(&self) -> zbus::Result<()>;
    fn open_preferences(&self) -> zbus::Result<()>;
    fn ping(&self) -> zbus::Result<String>;
}
