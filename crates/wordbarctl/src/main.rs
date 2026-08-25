//! `wordbarctl` — the CLI frontend and the backend's debugging tool.
//!
//! Every command is a D-Bus call. Starting the service is the session bus's
//! job: the first call activates it.

mod proxy;
mod word;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use zbus::Connection;

use proxy::{WordDict, WordbarProxy};
use word::Word;

/// Exit codes, so scripts and the test suite can branch without parsing text.
mod exit {
    pub const OK: i32 = 0;
    pub const ERROR: i32 = 1;
    pub const NOT_FOUND: i32 = 2;
    pub const DUPLICATE: i32 = 3;
    pub const UNAVAILABLE: i32 = 4;
}

#[derive(Parser)]
#[command(
    name = "wordbarctl",
    version,
    about = "Control the Wordbar vocabulary service",
    long_about = "Control the Wordbar vocabulary service.\n\n\
                  `current` is read-only and safe to poll; `next` changes state."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a word or short phrase to the vocabulary.
    Add {
        /// The term. Multiple arguments are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        #[command(flatten)]
        output: OutputArgs,
    },

    /// Show the current panel word. Read-only: polling never rotates.
    Current {
        #[command(flatten)]
        output: OutputArgs,
    },

    /// Rotate to the next word. Changes state.
    Next {
        #[command(flatten)]
        output: OutputArgs,
    },

    /// Rate the word "Again" (could not recall) and advance.
    Again {
        /// Term or id. Defaults to the current word.
        target: Option<String>,
    },

    /// Rate the word "Know" (recalled) and advance.
    Know {
        /// Term or id. Defaults to the current word.
        target: Option<String>,
    },

    /// Archive a word so it stops resurfacing, and advance.
    Learned {
        /// Term or id. Defaults to the current word.
        target: Option<String>,
    },

    /// Bring an archived word back into rotation.
    Restore {
        /// Term or id.
        target: String,
    },

    /// Remove a word and its history permanently.
    Delete {
        /// Term or id.
        target: String,
    },

    /// List the vocabulary.
    List {
        /// Substring to match against term, definition and example.
        query: Option<String>,
        /// Show archived words instead of active ones.
        #[arg(long, conflicts_with = "all")]
        learned: bool,
        /// Show both active and archived words.
        #[arg(long)]
        all: bool,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },

    /// Look a word up again, refreshing its senses and pronunciation.
    Refresh {
        /// Term or id. Omit with --all to refresh everything.
        target: Option<String>,
        /// Refresh every word, for backfilling after an upgrade.
        #[arg(long)]
        all: bool,
    },

    /// Show vocabulary counts.
    Stats {
        #[arg(long)]
        json: bool,
    },

    /// Write portable vocabulary to stdout.
    Export {
        /// json or csv.
        #[arg(long, default_value = "json")]
        format: String,
        #[arg(long, conflicts_with = "all")]
        learned: bool,
        #[arg(long)]
        all: bool,
    },

    /// Read portable vocabulary from a file, or from stdin with `-`.
    Import {
        /// Path to the file, or `-` for stdin.
        path: String,
        /// json or csv. Guessed from the filename when omitted.
        #[arg(long)]
        format: Option<String>,
    },

    /// Open the vocabulary manager.
    Vocabulary,

    /// Open the manager's preferences page.
    Preferences,

    /// Check that the service is reachable; prints its version.
    Ping,
}

#[derive(Args, Clone, Copy)]
struct OutputArgs {
    /// Emit JSON.
    #[arg(long, conflicts_with = "waybar")]
    json: bool,
    /// Emit a single line of Waybar-compatible JSON.
    #[arg(long)]
    waybar: bool,
}

impl OutputArgs {
    fn render(self, word: Option<&Word>) {
        match (self.waybar, self.json, word) {
            (true, _, _) => println!("{}", waybar_json(word)),
            (_, true, Some(w)) => println!("{}", serde_json::to_string(w).unwrap_or_default()),
            (_, true, None) => println!("null"),
            (_, false, Some(w)) => println!("{}", w.plain()),
            (_, false, None) => println!("{}", EMPTY_HINT),
        }
    }
}

const EMPTY_HINT: &str = "No words yet. Add one with: wordbarctl add <word>";

/// Waybar reads one JSON object per poll and breaks on anything else, so this
/// always succeeds — an empty vocabulary is a state, not an error.
fn waybar_json(word: Option<&Word>) -> String {
    #[derive(Serialize)]
    struct Waybar<'a> {
        text: &'a str,
        tooltip: String,
        class: &'a str,
    }

    let payload = match word {
        Some(w) => Waybar {
            text: &w.term,
            tooltip: tooltip(w),
            class: if w.definition.is_some() {
                "learning"
            } else {
                "incomplete"
            },
        },
        None => Waybar {
            text: "+ word",
            tooltip: EMPTY_HINT.to_string(),
            class: "empty",
        },
    };
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
}

fn tooltip(word: &Word) -> String {
    let mut lines = Vec::new();
    if let Some(pos) = &word.part_of_speech {
        lines.push(pos.clone());
    }
    lines.push(
        word.definition
            .clone()
            .unwrap_or_else(|| "Definition unavailable".to_string()),
    );
    if let Some(example) = &word.example {
        lines.push(String::new());
        lines.push(example.clone());
    }
    lines.join("\n")
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let code = match run(cli).await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("wordbarctl: {}", describe(&err));
            classify(&err)
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli) -> zbus::Result<i32> {
    let connection = match Connection::session().await {
        Ok(c) => c,
        Err(err) => {
            eprintln!("wordbarctl: no session bus: {err}");
            return Ok(exit::UNAVAILABLE);
        }
    };
    // The first call activates the service via its .service file.
    let wordbar = WordbarProxy::new(&connection).await?;

    match cli.command {
        Command::Add { text, output } => {
            let dict = wordbar.add_word(&text.join(" ")).await?;
            let word = Word::from_dict(&dict);
            if !output.json && !output.waybar {
                match &word {
                    Some(w) => println!("Added \u{201c}{}\u{201d}", w.term),
                    None => println!("Added"),
                }
            } else {
                output.render(word.as_ref());
            }
        }

        Command::Current { output } => {
            let dict = wordbar.get_current_word().await?;
            let word = Word::from_dict(&dict);
            output.render(word.as_ref());
            // --waybar must always exit 0 so the bar keeps rendering.
            if word.is_none() && !output.waybar {
                return Ok(exit::NOT_FOUND);
            }
        }

        Command::Next { output } => {
            let dict = wordbar.advance_display().await?;
            let word = Word::from_dict(&dict);
            output.render(word.as_ref());
            if word.is_none() && !output.waybar {
                return Ok(exit::NOT_FOUND);
            }
        }

        Command::Again { target } => {
            let Some(id) = resolve(&wordbar, target.as_deref()).await? else {
                return Ok(exit::NOT_FOUND);
            };
            report_next(wordbar.rate_again(id).await?);
        }

        Command::Know { target } => {
            let Some(id) = resolve(&wordbar, target.as_deref()).await? else {
                return Ok(exit::NOT_FOUND);
            };
            report_next(wordbar.rate_know(id).await?);
        }

        Command::Learned { target } => {
            let Some(id) = resolve(&wordbar, target.as_deref()).await? else {
                return Ok(exit::NOT_FOUND);
            };
            let term = term_of(&wordbar, id).await;
            let next = wordbar.mark_learned(id).await?;
            println!("Marked \u{201c}{term}\u{201d} as learned");
            report_next(next);
        }

        Command::Restore { target } => {
            let Some(id) = resolve(&wordbar, Some(&target)).await? else {
                return Ok(exit::NOT_FOUND);
            };
            let dict = wordbar.restore_word(id).await?;
            if let Some(word) = Word::from_dict(&dict) {
                println!("Restored \u{201c}{}\u{201d}", word.term);
            }
        }

        Command::Delete { target } => {
            let Some(id) = resolve(&wordbar, Some(&target)).await? else {
                return Ok(exit::NOT_FOUND);
            };
            let term = term_of(&wordbar, id).await;
            wordbar.delete_word(id).await?;
            println!("Deleted \u{201c}{term}\u{201d}");
        }

        Command::List {
            query,
            learned,
            all,
            json,
        } => {
            let filter = filter_name(learned, all);
            let dicts = wordbar
                .list_words(filter, query.as_deref().unwrap_or(""))
                .await?;
            let words: Vec<Word> = dicts.iter().filter_map(Word::from_dict).collect();

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&words).unwrap_or_default()
                );
            } else if words.is_empty() {
                println!("{EMPTY_HINT}");
                return Ok(exit::NOT_FOUND);
            } else {
                let width = words
                    .iter()
                    .map(|w| w.term.chars().count())
                    .max()
                    .unwrap_or(0);
                for w in &words {
                    match &w.definition {
                        Some(def) => println!("{:width$}  {}", w.term, def),
                        None => println!("{:width$}  \u{2014}", w.term),
                    }
                }
            }
        }

        Command::Refresh { target, all } => {
            let ids: Vec<i64> = if all {
                wordbar
                    .list_words("all", "")
                    .await?
                    .iter()
                    .filter_map(Word::from_dict)
                    .map(|w| w.id)
                    .collect()
            } else {
                match resolve(&wordbar, target.as_deref()).await? {
                    Some(id) => vec![id],
                    None => return Ok(exit::NOT_FOUND),
                }
            };

            let mut refreshed = 0;
            let mut failed = 0;
            for id in ids {
                let term = term_of(&wordbar, id).await;
                match wordbar.refresh_word(id).await {
                    Ok(senses) => {
                        println!("{term}: {senses} sense(s)");
                        refreshed += 1;
                    }
                    Err(err) => {
                        eprintln!("{term}: {}", describe(&err));
                        failed += 1;
                    }
                }
            }
            println!("Refreshed {refreshed}, {failed} failed");
            if refreshed == 0 && failed > 0 {
                return Ok(exit::ERROR);
            }
        }

        Command::Stats { json } => {
            let (active, learned) = wordbar.get_stats().await?;
            if json {
                println!("{{\"active\":{active},\"learned\":{learned}}}");
            } else {
                println!("{active} active \u{b7} {learned} learned");
            }
        }

        Command::Export {
            format,
            learned,
            all,
        } => {
            let data = wordbar
                .export_vocabulary(filter_name(learned, all), &format)
                .await?;
            print!("{data}");
        }

        Command::Import { path, format } => {
            let data = if path == "-" {
                std::io::read_to_string(std::io::stdin())
                    .map_err(|e| zbus::Error::Failure(e.to_string()))?
            } else {
                std::fs::read_to_string(&path)
                    .map_err(|e| zbus::Error::Failure(format!("{path}: {e}")))?
            };
            let format = format.unwrap_or_else(|| {
                if path.to_lowercase().ends_with(".csv") {
                    "csv".into()
                } else {
                    "json".into()
                }
            });
            let (added, duplicates, invalid) = wordbar.import_vocabulary(&data, &format).await?;
            println!("Imported {added}, skipped {duplicates} duplicate(s), {invalid} invalid");
        }

        Command::Vocabulary => wordbar.open_vocabulary().await?,
        Command::Preferences => wordbar.open_preferences().await?,
        Command::Ping => println!("wordbar-service {}", wordbar.ping().await?),
    }

    Ok(exit::OK)
}

fn filter_name(learned: bool, all: bool) -> &'static str {
    match (learned, all) {
        (_, true) => "all",
        (true, _) => "learned",
        _ => "active",
    }
}

fn report_next(dict: WordDict) {
    match Word::from_dict(&dict) {
        Some(word) => println!("Next: {}", word.term),
        None => println!("{EMPTY_HINT}"),
    }
}

/// Accept either a numeric id or a term; `None` means "the current word".
async fn resolve(wordbar: &WordbarProxy<'_>, target: Option<&str>) -> zbus::Result<Option<i64>> {
    let dict = match target {
        None => wordbar.get_current_word().await?,
        Some(text) => match text.parse::<i64>() {
            Ok(id) => wordbar.get_word_details(id).await?,
            Err(_) => wordbar.find_word(text).await?,
        },
    };

    match Word::from_dict(&dict) {
        Some(word) => Ok(Some(word.id)),
        None => {
            match target {
                Some(text) => {
                    eprintln!("wordbarctl: \u{201c}{text}\u{201d} is not in your vocabulary")
                }
                None => eprintln!("wordbarctl: {EMPTY_HINT}"),
            }
            Ok(None)
        }
    }
}

async fn term_of(wordbar: &WordbarProxy<'_>, id: i64) -> String {
    wordbar
        .get_word_details(id)
        .await
        .ok()
        .and_then(|d| Word::from_dict(&d))
        .map(|w| w.term)
        .unwrap_or_else(|| id.to_string())
}

/// Strip the D-Bus error name so users see the message, not the wire format.
fn describe(err: &zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(_, Some(message), _) => message.clone(),
        zbus::Error::MethodError(name, None, _) => name.to_string(),
        other => other.to_string(),
    }
}

fn classify(err: &zbus::Error) -> i32 {
    match err {
        zbus::Error::MethodError(name, ..) => match name.as_str() {
            "org.wordbar.Error.Duplicate" => exit::DUPLICATE,
            "org.wordbar.Error.NotFound" | "org.wordbar.Error.Empty" => exit::NOT_FOUND,
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.Spawn.ExecFailed" => exit::UNAVAILABLE,
            _ => exit::ERROR,
        },
        _ => exit::ERROR,
    }
}
