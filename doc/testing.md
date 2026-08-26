# Test matrix

Four layers, tested independently (Phase 27).

## 1. Rust unit tests — `cargo test -p wordbar-core`

| Area | Where |
|---|---|
| normalization | `crates/wordbar-core/src/normalize.rs` |
| FSRS mapping | `crates/wordbar-core/src/scheduler.rs` |
| display selector | `crates/wordbar-core/src/select.rs` |
| dictionary parsing | `crates/wordbar-service/src/dict/free_dictionary.rs` |
| import/export | `crates/wordbar-service/src/exchange.rs` |
| lookup retries | `crates/wordbar-service/src/enrich.rs`, `scripts/e2e-retry.sh` |

## 2. Storage and engine — `cargo test -p wordbar-service`

| Area | Where |
|---|---|
| add, duplicates, edit, delete, search | `tests/storage.rs` |
| learned/restore keeping FSRS state | `tests/storage.rs`, `tests/engine.rs` |
| display is not a review | `tests/engine.rs` |
| `current` is read-only, `next` rotates | `tests/engine.rs` |
| the full add → rate → learn → restore workflow | `tests/engine.rs` |
| reopening the database | `tests/storage.rs` |

## 3. D-Bus and CLI — `make e2e`

`scripts/e2e-backend.sh` runs on a private session bus against a throwaway
database, so it never touches your real vocabulary. It covers:

* activation — the bus starts the service, nothing else does
* the documented CLI workflow end to end
* plain, JSON and Waybar output shapes
* exit codes for duplicates, missing words and an empty vocabulary
* import/export round trip, and that exports carry no scheduler internals
* restart resilience — the bus re-activates and the data survives

## 4. GNOME Shell — manual

Automated Shell testing needs a live session, so this list is run by hand
before a release.

Use `make dev` rather than logging out between builds: it starts a nested
GNOME Shell in a window with a throwaway vocabulary, so a code change costs a
window close instead of a session. Add `--real-data` to test against your own
words. Log out and back in only for a final check in a real session, since a
nested Shell cannot verify the global capture shortcut.

- [ ] the panel word appears after enabling the extension
- [ ] with an empty vocabulary the panel shows `+ word` and the popup explains capture
- [ ] with the service stopped the panel shows `—` and the Shell stays responsive
- [ ] the panel recovers on its own once the service is reachable again
- [ ] clicking the word shows the definition immediately, with no second click
- [ ] a long definition wraps and makes the popup taller, never wider
- [ ] a word with no definition shows "Definition unavailable" and an "Add definition" link
- [ ] rotation advances the word after the configured interval
- [ ] rotation does **not** change the word while its popup is open
- [ ] closing the popup after a deferred rotation advances exactly once
- [ ] `Again`, `Know` and `Learned` each record and advance
- [ ] `Learned` shows a confirmation with a working Undo
- [ ] `Super+Shift+V` on a PRIMARY selection saves silently and confirms
- [ ] `Super+Shift+V` with no selection but a plausible clipboard opens a prefilled dialog
- [ ] `Super+Shift+V` on a long paragraph opens an editable dialog rather than saving
- [ ] capturing a duplicate says so and does not create a second entry
- [ ] Undo on a capture removes the word
- [ ] a captured word gains its definition shortly afterwards (online)
- [ ] with networking off, capture still succeeds and the word stays usable
- [ ] changing the rotation interval in preferences takes effect without a restart
- [ ] turning off "Show word in top bar" removes the indicator
- [ ] rebinding the capture shortcut works and the old binding stops firing
- [ ] disabling the extension removes the indicator, the timer and the keybinding
- [ ] a word with several senses shows a `1/4` stepper; stepping changes the definition
- [ ] the chosen sense survives a rotation away and back
- [ ] the reset arrow appears only after stepping, and returns to the opening sense
- [ ] closing and reopening the card makes the current sense the new reset point
- [ ] a word with one sense shows no stepper at all
- [ ] the speaker plays a real recording, and still says something without one
- [ ] the dot appears only after a whole interval passes unnoticed, and clears on opening
- [ ] an ignored word holds the panel, then moves on after two held intervals

## 5. GTK manager — manual

- [ ] the vocabulary list loads, searches and shows `N active · M learned`
- [ ] the Learning/Known toggle splits the list by review interval
- [ ] a word drilled until its interval is long appears under Known
- [ ] a known word still appears in the panel occasionally
- [ ] archiving by hand also files a word under Known, with a Restore action
- [ ] editing a word updates it and the panel picks the change up
- [ ] deleting asks first, then removes the word
- [ ] a word captured from the Shell appears in the list without a manual reload
- [ ] import accepts `word`, `word pos`, `word,definition` and the full CSV row
- [ ] a word imported while the dictionary is down gets its definition once it is back
- [ ] export writes JSON or CSV depending on the chosen filename
- [ ] with the service unreachable, the window shows an error toast rather than hanging
- [ ] the editor's sense chooser fills the fields, and Save keeps the choice
- [ ] "Choose sense…" on a row switches the sense in one step
- [ ] "Choose sense…" on a word with one sense says so rather than opening an empty dialog
- [ ] import and export are reachable from the window menu
- [ ] changing the capture shortcut here rebinds it in the Shell immediately
- [ ] the popup's "N words need definitions" hint opens the list filtered to them
- [ ] that filter shows a banner, and "Show all" leaves it
- [ ] searching or switching Active/Learned also leaves the filter
- [ ] right-click offers Add word, Vocabulary, Preferences and Quit
- [ ] with no words the popup shows no rating buttons at all
- [ ] Quit removes the indicator entirely, and it returns after a logout
- [ ] with no words the popup offers "+ Add word"
- [ ] the sense reset sits left of the arrows and never shifts them
