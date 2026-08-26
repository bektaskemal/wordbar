//! The enrichment worker: one queue, one lookup at a time, retried.
//!
//! Enrichment is best-effort and always happens after a word is saved, so a
//! failing dictionary can never fail a capture. The cost of that is that a
//! failed lookup used to be lost: the word stayed definitionless until someone
//! noticed and ran `refresh --all` by hand. A free dictionary API is down often
//! enough that "notice it yourself" is not a policy.
//!
//! So every request goes through a single worker that keeps what it could not
//! finish. Two kinds of failure, treated differently:
//!
//! * The provider **did not answer** -- offline, timeout, a 5xx. Transient and
//!   almost never specific to the word, so the whole queue pauses and backs
//!   off rather than burning one timeout per word to learn the same thing.
//! * The provider **answered "no entry"** -- a typo, a proper noun, a phrase no
//!   dictionary carries. Retrying that on a timer is pointless, so the term is
//!   remembered and left alone until the service restarts.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::Instant;
use wordbar_core::{DictionaryProvider, WordId};
use zbus::Connection;

use crate::engine::{Engine, Enrichment};

/// Gap between lookups. Sequential and paced on purpose: an import can be
/// hundreds of words, and a free API deserves better than a burst.
const PACE: Duration = Duration::from_millis(250);

/// How long to wait after the provider fails to answer, by consecutive
/// failure. Doubling from seconds to half an hour: a blip costs almost
/// nothing, and an outage lasting all afternoon is retried twice an hour
/// instead of hammering.
const BACKOFF: [Duration; 6] = [
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(10 * 60),
    Duration::from_secs(20 * 60),
    Duration::from_secs(30 * 60),
];

/// After this many consecutive failures on the *same* word it goes to the back
/// of the queue, so one pathological term cannot block everything behind it.
const MAX_HEAD_FAILURES: u32 = 3;

/// Safety net for words that fell out of the queue -- a service that has been
/// up for days should still notice them.
const SWEEP: Duration = Duration::from_secs(6 * 60 * 60);

fn backoff(consecutive_failures: u32) -> Duration {
    let index = (consecutive_failures.max(1) as usize - 1).min(BACKOFF.len() - 1);
    BACKOFF[index]
}

/// Requests enrichment. Cloneable and cheap; dropping every clone stops the
/// worker.
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<WordId>,
}

impl Handle {
    /// Ask for a word to be looked up. Returns immediately.
    pub fn request(&self, id: WordId) {
        // The worker outlives every caller in practice; a closed channel just
        // means the service is shutting down.
        let _ = self.tx.send(id);
    }

    pub fn request_all(&self, ids: impl IntoIterator<Item = WordId>) {
        for id in ids {
            self.request(id);
        }
    }
}

/// Start the worker. It runs until the service exits.
pub fn spawn(
    engine: Arc<Mutex<Engine>>,
    dictionary: Arc<dyn DictionaryProvider>,
    connection: Connection,
) -> Handle {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(
        Worker {
            engine,
            dictionary,
            connection,
            queue: VecDeque::new(),
            queued: HashSet::new(),
            absent: HashSet::new(),
            word_failures: HashMap::new(),
            failures: 0,
        }
        .run(rx),
    );
    Handle { tx }
}

struct Worker {
    engine: Arc<Mutex<Engine>>,
    dictionary: Arc<dyn DictionaryProvider>,
    connection: Connection,
    queue: VecDeque<WordId>,
    /// Mirrors `queue`, so a word asked for twice is looked up once.
    queued: HashSet<WordId>,
    /// Terms the provider has answered "no entry" for.
    absent: HashSet<String>,
    word_failures: HashMap<WordId, u32>,
    /// Consecutive failures to reach the provider at all.
    failures: u32,
}

/// What one attempt tells us about what to do next.
enum Attempt {
    /// Stored. The provider is reachable.
    Done,
    /// Nothing to do -- deleted, already complete, or known absent.
    Dropped,
    /// The provider did not answer.
    Unreachable,
}

impl Worker {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<WordId>) {
        // Words left incomplete by an earlier session, or by an outage that
        // outlasted it.
        self.sweep().await;
        let mut next_sweep = Instant::now() + SWEEP;

        loop {
            while let Ok(id) = rx.try_recv() {
                self.push_back(id);
            }

            let Some(id) = self.queue.pop_front() else {
                // Idle: wait for a request or the next sweep, whichever first.
                tokio::select! {
                    received = rx.recv() => match received {
                        Some(id) => self.push_back(id),
                        // Every handle dropped: the service is going away.
                        None => return,
                    },
                    _ = tokio::time::sleep_until(next_sweep) => {
                        self.sweep().await;
                        next_sweep = Instant::now() + SWEEP;
                    }
                }
                continue;
            };
            self.queued.remove(&id);

            match self.attempt(id).await {
                Attempt::Done => {
                    self.failures = 0;
                    self.word_failures.remove(&id);
                    tokio::time::sleep(PACE).await;
                }
                Attempt::Dropped => {
                    self.word_failures.remove(&id);
                }
                Attempt::Unreachable => {
                    self.failures += 1;
                    let word_failures = self.word_failures.entry(id).or_insert(0);
                    *word_failures += 1;

                    if *word_failures >= MAX_HEAD_FAILURES {
                        self.push_back(id);
                    } else {
                        self.push_front(id);
                    }

                    let wait = backoff(self.failures);
                    tracing::info!(
                        failures = self.failures,
                        seconds = wait.as_secs(),
                        pending = self.queue.len(),
                        "dictionary unreachable; retrying later"
                    );
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    /// Look one word up and store what came back.
    async fn attempt(&mut self, id: WordId) -> Attempt {
        let term = {
            let engine = self.engine.lock().await;
            match engine.get_word(id) {
                // Nothing more to fill in, or the user fixed it by hand.
                Ok(word) if !word.is_incomplete() => return Attempt::Dropped,
                Ok(word) => word.term.clone(),
                // Deleted while it sat in the queue.
                Err(_) => return Attempt::Dropped,
            }
        };

        if self.absent.contains(&term) {
            return Attempt::Dropped;
        }

        let entry = match self.dictionary.lookup(&term).await {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                tracing::debug!(%term, "no dictionary entry; not retrying");
                self.absent.insert(term);
                return Attempt::Dropped;
            }
            Err(err) => {
                tracing::debug!(%term, %err, "lookup failed");
                return Attempt::Unreachable;
            }
        };

        let effects = {
            let mut engine = self.engine.lock().await;
            match engine.apply_enrichment_with(id, &entry, Utc::now(), Enrichment::FillGaps) {
                Ok(outcome) => outcome.effects,
                Err(err) => {
                    tracing::info!(id, %err, "enrichment update skipped");
                    return Attempt::Dropped;
                }
            }
        };

        crate::dbus::publish(&self.connection, &self.engine, &effects).await;
        Attempt::Done
    }

    /// Queue every word still missing a definition.
    async fn sweep(&mut self) {
        let ids: Vec<WordId> = {
            let engine = self.engine.lock().await;
            match engine.list_incomplete() {
                Ok(words) => words
                    .into_iter()
                    .filter(|w| !self.absent.contains(&w.term))
                    .map(|w| w.id)
                    .collect(),
                Err(err) => {
                    tracing::info!(%err, "could not list incomplete words");
                    return;
                }
            }
        };

        if ids.is_empty() {
            return;
        }
        tracing::info!(count = ids.len(), "looking up words missing definitions");
        for id in ids {
            self.push_back(id);
        }
    }

    fn push_back(&mut self, id: WordId) {
        if self.queued.insert(id) {
            self.queue.push_back(id);
        }
    }

    fn push_front(&mut self, id: WordId) {
        if self.queued.insert(id) {
            self.queue.push_front(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_grows_and_then_holds() {
        assert_eq!(backoff(0), BACKOFF[0], "a first failure waits the least");
        assert_eq!(backoff(1), BACKOFF[0]);
        assert_eq!(backoff(2), BACKOFF[1]);
        assert_eq!(backoff(BACKOFF.len() as u32), *BACKOFF.last().unwrap());
        assert_eq!(
            backoff(10_000),
            *BACKOFF.last().unwrap(),
            "an outage never backs off past the cap"
        );
    }

    #[test]
    fn the_backoff_only_ever_increases() {
        for pair in BACKOFF.windows(2) {
            assert!(pair[1] > pair[0], "{pair:?} is not increasing");
        }
    }
}
