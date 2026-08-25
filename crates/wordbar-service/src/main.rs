//! D-Bus activated Wordbar service.
//!
//! Started on demand by the session bus (see
//! `packaging/dbus/org.wordbar.Wordbar.service`); it is never autostarted and
//! never spawned by the GNOME extension. Once activated it stays alive until
//! logout, which costs almost nothing because it idles between requests.

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;
use wordbar_service::dbus::{WordbarService, BUS_NAME, OBJECT_PATH};
use wordbar_service::{dict, paths, Engine, SqliteStore};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("WORDBAR_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let db_path = paths::database_path();
    tracing::info!(path = %db_path.display(), "opening database");
    let store =
        SqliteStore::open(&db_path).with_context(|| format!("opening {}", db_path.display()))?;

    let engine = Arc::new(Mutex::new(Engine::new(Box::new(store))));
    let dictionary = dict::default_provider();

    // Build the connection first: the interface needs it to emit signals, and
    // the bus name is only claimed once the object is being served, so no
    // client can observe a half-initialised service.
    let connection = zbus::conn::Builder::session()
        .context("connecting to the session bus")?
        .build()
        .await
        .context("connecting to the session bus")?;

    // Woken by the Quit method, which the panel's context menu offers.
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let service = WordbarService::new(
        engine,
        dictionary,
        connection.clone(),
        Arc::clone(&shutdown),
    );
    connection
        .object_server()
        .at(OBJECT_PATH, service)
        .await
        .context("serving the Wordbar object")?;
    connection
        .request_name(BUS_NAME)
        .await
        .with_context(|| format!("claiming {BUS_NAME}"))?;

    tracing::info!(name = BUS_NAME, path = OBJECT_PATH, "wordbar service ready");

    // Exit cleanly on SIGTERM/SIGINT so a session logout does not leave a
    // half-written WAL behind.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("installing the SIGTERM handler")?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("interrupted"),
        _ = sigterm.recv() => tracing::info!("terminated"),
        _ = shutdown.notified() => tracing::info!("stopped on request"),
    }
    Ok(())
}
