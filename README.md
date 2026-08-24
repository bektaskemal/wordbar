# Wordbar

One vocabulary word in the GNOME top bar. Select a word anywhere, press
`Super+Shift+V`, and Wordbar keeps resurfacing it until you know it.

```text
Activities            ephemeral            Wi-Fi  Battery
```

The panel word is the prompt. Clicking it reveals the answer and the three
things you can say about it:

```text
┌───────────────────────────────────┐
│ ephemeral                     🔊  │
│ adjective                         │
│                                   │
│ lasting for a very short time     │
│                                   │
│ The opportunity was ephemeral.    │
│                                   │
│ [ Again ]  [ Know ]  [ Learned ]  │
│                                   │
│ Next word                      →  │
├───────────────────────────────────┤
│ + Add word                     ⋮  │
└───────────────────────────────────┘
```

## Architecture

```text
GNOME Shell extension (GJS)     wordbarctl (CLI)      wordbar-app (GTK4)
            │                          │                      │
            └──────────── async D-Bus ─┴──────────────────────┘
                                  │
                       org.wordbar.Wordbar
                                  │
                        ┌─────────┴─────────┐
                        │  wordbar-service  │
                        │  SQLite (1 writer)│
                        │  rs-fsrs          │
                        │  dictionary       │
                        └───────────────────┘
```

Rules that keep it honest:

* Rust owns all application state; SQLite has exactly one writer.
* No frontend ever touches the database — not the extension, not the CLI.
* Every D-Bus call from GNOME Shell is asynchronous.
* Showing a word is **not** a review. Only `Again` and `Know` touch FSRS.
* `Learned` archives a word without touching its schedule, so restoring it
  resumes where it left off.

## Crates

| Crate | Role |
|---|---|
| `wordbar-core` | domain model, normalization, scheduler, selection policy, storage/dictionary traits |
| `wordbar-service` | SQLite, D-Bus API, background enrichment |
| `wordbarctl` | CLI D-Bus client; also the backend's debugging tool |
| `wordbar-app` | GTK4/libadwaita vocabulary manager and preferences |
| `extension/` | GNOME Shell extension (plain modern GJS) |

## Install

Needs a Rust toolchain, `libgtk-4-dev` and `libadwaita-1-dev`.

```bash
make install                       # into ~/.local by default
gnome-extensions enable wordbar@kemalbektas.dev
```

Log out and back in first (GNOME Shell only picks up new extensions and
GSettings schemas at session start). `make install PREFIX=/usr/local` does a
system-wide install.

The service is **D-Bus activated**: the session bus starts it the first time
anything calls it. There is no autostart entry, no systemd unit and nothing for
the extension to spawn.

## Using it

```bash
wordbarctl add ephemeral
wordbarctl current                 # read-only, safe to poll
wordbarctl current --json
wordbarctl current --waybar
wordbarctl next                    # rotates
wordbarctl again                   # FSRS Again, then advance
wordbarctl know                    # FSRS Good, then advance
wordbarctl learned                 # archive, then advance
wordbarctl restore ephemeral
wordbarctl list --learned
wordbarctl export --all > vocabulary.json
wordbarctl import vocabulary.csv
```

Exit codes: `0` success, `1` error, `2` not found / nothing to show,
`3` duplicate, `4` service unavailable.

Waybar users: see [doc/waybar.md](doc/waybar.md).

## Data

```text
~/.local/share/wordbar/wordbar.db
```

Export is *portable vocabulary* — terms, definitions, examples, parts of
speech. It deliberately contains no scheduler state, so an export is "my
words", not "my Wordbar installation". Full backup/restore of cards and review
history is a later feature; the schema keeps it possible.

## Development

```bash
make test    # Rust unit and integration tests
make e2e     # full backend over a private session bus
make lint    # rustfmt + clippy
```

`make e2e` is the milestone that matters: if `wordbarctl` cannot drive the
whole workflow reliably, no frontend work should start.

Environment variables useful while developing:

| Variable | Effect |
|---|---|
| `WORDBAR_DATA_DIR` | use a throwaway database |
| `WORDBAR_DICTIONARY=off` | disable enrichment (offline testing) |
| `WORDBAR_DICTIONARY_ENDPOINT` | point at a different dictionary API |
| `WORDBAR_LOG` | `tracing` filter, e.g. `debug` |
| `WORDBAR_APP` | binary used by `OpenVocabulary` / `OpenPreferences` |

See [doc/testing.md](doc/testing.md) for the release test matrix and
[doc/plan.md](doc/plan.md) for the original design.
