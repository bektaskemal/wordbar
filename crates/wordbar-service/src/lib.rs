//! Wordbar service: the single owner of application state.
//!
//! Everything reachable from the outside goes through the D-Bus interface in
//! [`dbus`]. SQLite is opened here and nowhere else.

pub mod dbus;
pub mod dict;
pub mod engine;
pub mod exchange;
pub mod paths;
pub mod store;

pub use engine::Engine;
pub use store::SqliteStore;
