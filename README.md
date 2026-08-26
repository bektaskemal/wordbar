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
| `wordbar-app` | GTK4/libadwaita vocabulary manager, and every setting |
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
wordbarctl refresh ephemeral       # look it up again
wordbarctl refresh --missing       # only the words with no definition
wordbarctl refresh --all           # backfill senses after an upgrade
wordbarctl list --known
wordbarctl export --all > vocabulary.json
wordbarctl import vocabulary.csv
```

Exit codes: `0` success, `1` error, `2` not found / nothing to show,
`3` duplicate, `4` service unavailable.

Waybar users: see [doc/waybar.md](doc/waybar.md).

## Importing a word list

Import takes JSON or CSV, and CSV is deliberately forgiving -- a plain list of
words is a valid file:

```
nonchalant adj
at large phrase
laconic
frugal,using few words
```

Line by line: a word with a part-of-speech tag, a multi-word term with one, a
bare word, and a full CSV row. An optional
`term,definition,example,part_of_speech` header is recognised and skipped.

A trailing `n`, `v`, `adj`, `adv`, `prep`, `conj`, `pron`, `interj`, `det`,
`abbr` or `phrase` is read as a part of speech and expanded (`adj` ->
`adjective`). It is only split off when the line has no other column, so a term
that merely ends in a short word survives intact.

The tag is not decoration: it decides which sense Wordbar shows. `haphazard
adv` picks the adverb sense rather than the more common adjective, where the
dictionary has one. Definitions are always left to the lookup -- supply them
only if you want your own.

Duplicates are skipped, never overwritten, and a bad row can't abort the
import; the CLI reports all three counts.

## Where definitions come from

Two sources, tried in order, neither needing a key or an account:

1. [dictionaryapi.dev](https://dictionaryapi.dev) -- concise, learner-shaped
   definitions, and the only one of the two that carries pronunciation
   recordings.
2. [Wiktionary](https://en.wiktionary.org) -- wordier and without audio, but run
   by the Wikimedia Foundation and correspondingly hard to knock over. It also
   covers phrases and rarer words the first one does not have.

The first is the better read, so it leads; the second is the reason a word
still gets a definition when the first is having a bad day. `refresh` re-asks
the chain from the top, so a word that fell back can be upgraded later.

A lookup that fails is not lost either: every one goes through a single worker
that keeps what it could not finish and tries again, backing off from 15
seconds to half an hour so an outage is retried twice an hour rather than
hammered. Words still missing a definition are picked up again when the service
starts, so an outage that outlasts a logout is covered too.

A word every source *answers* about but none has -- a typo, a name -- is not
retried on a timer, since the answer would not change. Those
show up in the manager's incomplete list, and fixing the spelling queues a
fresh lookup. To force one:

```bash
wordbarctl refresh --missing
```

## Importing from a Kindle

The Kindle's Vocabulary Builder keeps every word you looked up in
`system/vocabulary/vocab.db` on the device -- `system` is hidden, so turn on
hidden files after plugging it in. Copy that file off and convert it:

```bash
./scripts/kindle-vocab.py vocab.db > kindle.csv
wordbarctl import kindle.csv
```

Definitions are left to Wordbar, which looks each word up itself. What the
Kindle contributes is the sentence you met the word in, kept as the example --
the part no dictionary can give you. Words are exported in their base form, and
ones you marked as mastered on the device are skipped; `--help` covers the rest.

## Learning and known

The vocabulary splits itself. A word counts as **known** once FSRS has pushed
its review interval past two months, so the list reflects what you actually
remember rather than what you got round to marking. Known words stay in the
rotation -- rarely, as the schedule dictates -- because knowing a word is no
reason to stop testing it. Marking a word learned by hand is the separate,
stronger action: it takes the word out of rotation entirely.

## Definitions and pronunciation

Every sense the dictionary offers is stored, ranked so the word's dominant
part of speech leads -- otherwise `ephemeral` arrives as a noun, because that
is how the source lists it. The popup shows a `1/4` stepper when there is a
real choice, and picking a different sense keeps it.

Pronunciation prefers the dictionary's own recording, cached under
`~/.local/share/wordbar/audio`, and falls back to speech synthesis when there
is none. Playback happens in the service, never in the Shell.

Words captured before senses existed have none until you run
`wordbarctl refresh --all`.

## Settings

Rotation interval and capture shortcut live in the manager app, and import and
export sit with the vocabulary in the window's menu, so there is one place to
look. It writes the
extension's GSettings, so the panel applies changes immediately. The
extension's own preferences page just points at it.

## Data

```text
~/.local/share/wordbar/wordbar.db     vocabulary, senses, schedule, history
~/.local/share/wordbar/audio/         cached pronunciations
```

Export is *portable vocabulary* — terms, definitions, examples, parts of
speech. It deliberately contains no scheduler state, so an export is "my
words", not "my Wordbar installation". Full backup/restore of cards and review
history is a later feature; the schema keeps it possible.

## Development

```bash
make dev     # reload the extension in a nested Shell -- no logout
make test    # Rust and extension tests, plus static extension checks
make e2e     # full backend over a private session bus
make lint    # rustfmt + clippy
```

### Iterating on the extension

GNOME Shell is the Wayland compositor, so it cannot be restarted in place
(`Alt+F2` -> `r` is X11-only), and it imports each extension module exactly
once per process -- disabling and re-enabling will not pick up edited files.
A naive loop therefore costs a logout per change.

`make dev` avoids that by starting a second GNOME Shell nested in a window,
against a private session bus and a throwaway vocabulary. Edit, close the
window, run it again. Pass `--real-data` to use your actual vocabulary:

```bash
./scripts/dev-shell.sh --real-data
```

`make test` runs `scripts/check-extension.sh`, which catches the faults that
would otherwise cost a logout to discover: `gettext` or other calls at module
scope (they run before the extension object exists), unresolved relative
imports, malformed metadata, and a schema that does not compile.

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
