//! D-Bus client for the manager.
//!
//! The service is async (tokio) and GTK is not, so all D-Bus work happens on a
//! dedicated worker thread with its own runtime. The UI sends a request plus a
//! reply channel and awaits it with `glib::spawn_future_local`, which keeps the
//! window responsive without a single blocking call on the main thread.

use std::collections::HashMap;

use async_channel::{Receiver, Sender};
use zbus::proxy;

use crate::word::{Word, WordDict};

#[proxy(
    interface = "org.wordbar.Wordbar",
    default_service = "org.wordbar.Wordbar",
    default_path = "/org/wordbar/Wordbar"
)]
trait Wordbar {
    fn add_word(&self, text: &str) -> zbus::Result<WordDict>;
    fn get_word_details(&self, id: i64) -> zbus::Result<WordDict>;
    fn update_word(&self, id: i64, fields: HashMap<&str, &str>) -> zbus::Result<WordDict>;
    fn mark_learned(&self, id: i64) -> zbus::Result<WordDict>;
    fn restore_word(&self, id: i64) -> zbus::Result<WordDict>;
    fn delete_word(&self, id: i64) -> zbus::Result<()>;
    fn list_words(&self, filter: &str, query: &str) -> zbus::Result<Vec<WordDict>>;
    fn get_stats(&self) -> zbus::Result<(u32, u32)>;
    fn export_vocabulary(&self, filter: &str, format: &str) -> zbus::Result<String>;
    fn import_vocabulary(&self, data: &str, format: &str) -> zbus::Result<(u32, u32, u32)>;

    #[zbus(signal)]
    fn vocabulary_changed(&self) -> zbus::Result<()>;
}

/// Fields the edit dialog can change. Empty string clears a field.
#[derive(Debug, Clone, Default)]
pub struct EditFields {
    pub term: String,
    pub definition: String,
    pub example: String,
    pub part_of_speech: String,
}

#[derive(Debug)]
enum Request {
    List {
        filter: String,
        query: String,
        reply: Sender<Result<Vec<Word>, String>>,
    },
    Stats {
        reply: Sender<Result<(u32, u32), String>>,
    },
    Get {
        id: i64,
        reply: Sender<Result<Word, String>>,
    },
    Add {
        text: String,
        reply: Sender<Result<Word, String>>,
    },
    Update {
        id: i64,
        fields: EditFields,
        reply: Sender<Result<Word, String>>,
    },
    MarkLearned {
        id: i64,
        reply: Sender<Result<(), String>>,
    },
    Restore {
        id: i64,
        reply: Sender<Result<(), String>>,
    },
    Delete {
        id: i64,
        reply: Sender<Result<(), String>>,
    },
    Export {
        filter: String,
        format: String,
        reply: Sender<Result<String, String>>,
    },
    Import {
        data: String,
        format: String,
        reply: Sender<Result<(u32, u32, u32), String>>,
    },
}

#[derive(Clone)]
pub struct Client {
    requests: Sender<Request>,
    /// Fires whenever the service reports that the vocabulary changed.
    changes: Receiver<()>,
}

impl Client {
    /// Start the worker thread. Returns immediately; the connection is
    /// established in the background and the first call activates the service.
    pub fn start() -> Self {
        let (requests_tx, requests_rx) = async_channel::unbounded::<Request>();
        let (changes_tx, changes_rx) = async_channel::unbounded::<()>();

        std::thread::Builder::new()
            .name("wordbar-dbus".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime");
                runtime.block_on(worker(requests_rx, changes_tx));
            })
            .expect("spawning the D-Bus worker thread");

        Self {
            requests: requests_tx,
            changes: changes_rx,
        }
    }

    /// Receiver that yields once per `VocabularyChanged` signal.
    pub fn changes(&self) -> Receiver<()> {
        self.changes.clone()
    }

    async fn request<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, String>>) -> Request,
    ) -> Result<T, String> {
        let (tx, rx) = async_channel::bounded(1);
        self.requests
            .send(make(tx))
            .await
            .map_err(|_| "the Wordbar worker stopped".to_string())?;
        rx.recv()
            .await
            .map_err(|_| "no reply from the Wordbar service".to_string())?
    }

    pub async fn list(&self, filter: &str, query: &str) -> Result<Vec<Word>, String> {
        let (filter, query) = (filter.to_string(), query.to_string());
        self.request(|reply| Request::List {
            filter,
            query,
            reply,
        })
        .await
    }

    pub async fn stats(&self) -> Result<(u32, u32), String> {
        self.request(|reply| Request::Stats { reply }).await
    }

    /// Fetch one word by id, for opening its editor directly.
    pub async fn get(&self, id: i64) -> Result<Word, String> {
        self.request(|reply| Request::Get { id, reply }).await
    }

    pub async fn add(&self, text: &str) -> Result<Word, String> {
        let text = text.to_string();
        self.request(|reply| Request::Add { text, reply }).await
    }

    pub async fn update(&self, id: i64, fields: EditFields) -> Result<Word, String> {
        self.request(|reply| Request::Update { id, fields, reply })
            .await
    }

    pub async fn mark_learned(&self, id: i64) -> Result<(), String> {
        self.request(|reply| Request::MarkLearned { id, reply })
            .await
    }

    pub async fn restore(&self, id: i64) -> Result<(), String> {
        self.request(|reply| Request::Restore { id, reply }).await
    }

    pub async fn delete(&self, id: i64) -> Result<(), String> {
        self.request(|reply| Request::Delete { id, reply }).await
    }

    pub async fn export(&self, filter: &str, format: &str) -> Result<String, String> {
        let (filter, format) = (filter.to_string(), format.to_string());
        self.request(|reply| Request::Export {
            filter,
            format,
            reply,
        })
        .await
    }

    pub async fn import(&self, data: &str, format: &str) -> Result<(u32, u32, u32), String> {
        let (data, format) = (data.to_string(), format.to_string());
        self.request(|reply| Request::Import {
            data,
            format,
            reply,
        })
        .await
    }
}

async fn worker(requests: Receiver<Request>, changes: Sender<()>) {
    let connection = match zbus::Connection::session().await {
        Ok(connection) => connection,
        Err(err) => {
            drain_with_error(requests, format!("cannot reach the session bus: {err}")).await;
            return;
        }
    };

    let proxy = match WordbarProxy::new(&connection).await {
        Ok(proxy) => proxy,
        Err(err) => {
            drain_with_error(requests, describe(&err)).await;
            return;
        }
    };

    // Keep the list in sync with captures made from the Shell.
    if let Ok(mut stream) = proxy.receive_vocabulary_changed().await {
        let changes = changes.clone();
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            while stream.next().await.is_some() {
                if changes.send(()).await.is_err() {
                    break;
                }
            }
        });
    }

    while let Ok(request) = requests.recv().await {
        handle(&proxy, request).await;
    }
}

async fn handle(proxy: &WordbarProxy<'_>, request: Request) {
    match request {
        Request::List {
            filter,
            query,
            reply,
        } => {
            let result = proxy
                .list_words(&filter, &query)
                .await
                .map(|dicts| dicts.iter().filter_map(Word::from_dict).collect())
                .map_err(|e| describe(&e));
            let _ = reply.send(result).await;
        }
        Request::Stats { reply } => {
            let _ = reply
                .send(proxy.get_stats().await.map_err(|e| describe(&e)))
                .await;
        }
        Request::Get { id, reply } => {
            let result = proxy
                .get_word_details(id)
                .await
                .map_err(|e| describe(&e))
                .and_then(|d| Word::from_dict(&d).ok_or_else(|| format!("no word with id {id}")));
            let _ = reply.send(result).await;
        }
        Request::Add { text, reply } => {
            let result = proxy
                .add_word(&text)
                .await
                .map_err(|e| describe(&e))
                .and_then(|d| Word::from_dict(&d).ok_or_else(|| "empty reply".to_string()));
            let _ = reply.send(result).await;
        }
        Request::Update { id, fields, reply } => {
            let mut map: HashMap<&str, &str> = HashMap::new();
            map.insert("term", fields.term.as_str());
            map.insert("definition", fields.definition.as_str());
            map.insert("example", fields.example.as_str());
            map.insert("part_of_speech", fields.part_of_speech.as_str());
            let result = proxy
                .update_word(id, map)
                .await
                .map_err(|e| describe(&e))
                .and_then(|d| Word::from_dict(&d).ok_or_else(|| "empty reply".to_string()));
            let _ = reply.send(result).await;
        }
        Request::MarkLearned { id, reply } => {
            let _ = reply
                .send(
                    proxy
                        .mark_learned(id)
                        .await
                        .map(|_| ())
                        .map_err(|e| describe(&e)),
                )
                .await;
        }
        Request::Restore { id, reply } => {
            let _ = reply
                .send(
                    proxy
                        .restore_word(id)
                        .await
                        .map(|_| ())
                        .map_err(|e| describe(&e)),
                )
                .await;
        }
        Request::Delete { id, reply } => {
            let _ = reply
                .send(proxy.delete_word(id).await.map_err(|e| describe(&e)))
                .await;
        }
        Request::Export {
            filter,
            format,
            reply,
        } => {
            let _ = reply
                .send(
                    proxy
                        .export_vocabulary(&filter, &format)
                        .await
                        .map_err(|e| describe(&e)),
                )
                .await;
        }
        Request::Import {
            data,
            format,
            reply,
        } => {
            let _ = reply
                .send(
                    proxy
                        .import_vocabulary(&data, &format)
                        .await
                        .map_err(|e| describe(&e)),
                )
                .await;
        }
    }
}

/// If the worker cannot start, every pending and future request must still get
/// an answer so the UI shows an error instead of hanging.
async fn drain_with_error(requests: Receiver<Request>, message: String) {
    while let Ok(request) = requests.recv().await {
        let message = message.clone();
        match request {
            Request::List { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Stats { reply } => drop(reply.send(Err(message)).await),
            Request::Get { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Add { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Update { reply, .. } => drop(reply.send(Err(message)).await),
            Request::MarkLearned { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Restore { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Delete { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Export { reply, .. } => drop(reply.send(Err(message)).await),
            Request::Import { reply, .. } => drop(reply.send(Err(message)).await),
        }
    }
}

/// Show the service's message, not the D-Bus wire name.
pub fn describe(err: &zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(_, Some(message), _) => message.clone(),
        zbus::Error::MethodError(name, None, _) => name.to_string(),
        other => other.to_string(),
    }
}
