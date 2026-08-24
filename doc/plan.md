# Wordbar — Final Step-by-Step Implementation Plan

## 0. Lock the architecture

Build Wordbar as three pieces:

```text
GNOME Shell extension (plain GJS)
        │
        │ async D-Bus
        ▼
Rust Wordbar service
        │
        ├── wordbar-core
        ├── SQLite
        ├── rs-fsrs
        └── dictionary enrichment

wordbarctl
        │
        └── async D-Bus → same service
```

And separately:

```text
Rust GTK4/libadwaita app
        │
        └── vocabulary/settings UI
```

Core rules:

* Rust owns all application/domain state.
* SQLite has one writer: the Rust service.
* GJS never accesses SQLite directly.
* `wordbarctl` never accesses SQLite directly.
* GNOME Shell never performs synchronous D-Bus/network/file operations.
* Passive display is not an FSRS review.
* `Again` → FSRS Again.
* `Know` → FSRS Good.
* `Learned` → archive from normal rotation.

---

# Phase 1 — Create the Rust workspace

Start with:

```text
wordbar/
├── Cargo.toml
├── crates/
│   ├── wordbar-core/
│   ├── wordbar-service/
│   ├── wordbarctl/
│   └── wordbar-app/
├── extension/
└── packaging/
```

Responsibilities:

```text
wordbar-core
    vocabulary models
    normalization
    scheduler
    selection logic
    storage abstraction
    dictionary abstraction

wordbar-service
    SQLite
    D-Bus API
    background enrichment

wordbarctl
    CLI D-Bus client

wordbar-app
    GTK/libadwaita manager

extension
    GNOME Shell UI
```

Do not start with GTK or GNOME UI yet.

---

# Phase 2 — Define the domain model

Create the basic Rust types.

## Word

```rust
struct Word {
    id: WordId,
    term: String,
    normalized_term: String,

    definition: Option<String>,
    example: Option<String>,
    part_of_speech: Option<String>,
    phonetic: Option<String>,

    status: WordStatus,

    created_at: DateTime,
    updated_at: DateTime,
    last_displayed_at: Option<DateTime>,
}
```

Statuses:

```rust
enum WordStatus {
    Active,
    Learned,
}
```

Do not physically delete words immediately unless explicitly requested.

## Review state

Store the state required by `rs-fsrs` separately.

Conceptually:

```text
Card
────
word_id
due
stability
difficulty
state
reps
lapses
last_review
```

## Review record

```text
Review
──────
id
word_id
rating
reviewed_at
```

Keep review history from the beginning. It will help with debugging, undo, migrations, and future scheduler changes.

---

# Phase 3 — Implement normalization

Before touching FSRS, make adding words reliable.

Implement:

```text
normalize_term()
```

Rules:

* trim whitespace
* collapse repeated spaces
* Unicode-normalize
* strip surrounding quotes
* strip obvious surrounding punctuation
* lowercase only for comparison
* retain original display capitalization

Examples:

```text
“Ephemeral”
ephemeral
EPHEMERAL
```

should resolve to the same normalized key.

Support short phrases:

```text
ad hoc
by and large
rule of thumb
```

Initial validation:

```text
1–5 words
≤80 characters
```

Add unit tests immediately.

---

# Phase 4 — Add SQLite

Use one local database:

```text
~/.local/share/wordbar/wordbar.db
```

Create initial migrations.

Tables:

```text
words
cards
reviews
```

Optionally:

```text
meta
```

for database/schema version.

Implement repository methods:

```text
add_word()
find_by_term()
get_word()
update_word()
mark_learned()
restore_word()
delete_word()

list_active()
list_learned()
search_words()
```

At the end of this phase, write Rust tests proving:

```text
add
duplicate detection
edit
learned
restore
delete
search
```

all work without any UI.

---

# Phase 5 — Integrate `rs-fsrs`

Do not write custom spaced-repetition mathematics.

Wrap `rs-fsrs` behind our own interface:

```rust
trait Scheduler {
    fn again(...);
    fn know(...);
}
```

Wordbar mapping:

```text
Again
→ FSRS Again

Know
→ FSRS Good

Learned
→ no FSRS operation
→ WordStatus::Learned
```

Important invariant:

```text
displaying a word
≠ review
```

Only explicit `Again` or `Know` updates FSRS.

Write tests such as:

```text
new word
→ Again
→ receives relatively near due date

new word
→ Know repeatedly
→ due interval grows
```

Do not expose FSRS settings to users.

---

# Phase 6 — Implement Wordbar's display selector

FSRS answers:

> When is this card due?

Wordbar separately answers:

> Which word should appear in the panel now?

Create:

```rust
select_display_word()
```

Initial policy:

```text
1. active words only
2. exclude current word if possible
3. strongly prefer FSRS-due words
4. boost recently added words
5. penalize recently displayed words
6. add small randomness
```

Do not always choose the single most overdue card.

Record:

```text
last_displayed_at
```

when a word becomes the current panel word.

Add tests to ensure:

* no immediate repetition when alternatives exist
* learned words never appear
* overdue words are favored
* new words surface reasonably quickly

---

# Phase 7 — Build the service API before GNOME

Create the Rust D-Bus service.

Name:

```text
org.wordbar.Wordbar
```

Initial API:

```text
AddWord(text)

GetCurrentWord()
AdvanceDisplay()
GetWordDetails(id)

RateAgain(id)
RateKnow(id)
MarkLearned(id)
RestoreWord(id)

ListWords(...)
DeleteWord(id)

OpenVocabulary()
OpenPreferences()
```

Signals:

```text
CurrentWordChanged
WordAdded
WordUpdated
VocabularyChanged
```

Keep D-Bus objects simple.

Do not expose raw SQL or raw FSRS internals.

---

# Phase 8 — Make the service D-Bus activated

Install:

```text
/usr/share/dbus-1/services/org.wordbar.Wordbar.service
```

Desired lifecycle:

```text
client calls Wordbar
       ↓
service isn't running
       ↓
session D-Bus starts it
       ↓
service claims bus name
       ↓
request completes
```

No:

* autostart daemon
* extension-spawned binary
* manually managed background process

For v1, once activated, allow the lightweight service to remain alive until logout.

---

# Phase 9 — Implement `wordbarctl`

Do this before GNOME Shell.

It becomes both a user feature and the main backend debugging tool.

Commands:

```bash
wordbarctl add ephemeral

wordbarctl current
wordbarctl current --json
wordbarctl current --waybar

wordbarctl next

wordbarctl again
wordbarctl know
wordbarctl learned

wordbarctl restore ephemeral
wordbarctl delete ephemeral

wordbarctl list
wordbarctl list --learned
```

Every command talks to D-Bus.

Never:

```text
wordbarctl → SQLite
```

## Waybar output

`current --waybar`:

```json
{
  "text": "ephemeral",
  "tooltip": "lasting for a very short time",
  "class": "learning"
}
```

Important:

```text
current
```

is read-only.

```text
next
```

changes state.

Waybar should therefore poll `current --waybar`, not `next`.

---

# Phase 10 — End-to-end backend test

At this point, before writing GNOME code, this should work:

```bash
wordbarctl add ephemeral
wordbarctl current
wordbarctl again
wordbarctl next
wordbarctl know
wordbarctl learned
wordbarctl list --learned
wordbarctl restore ephemeral
```

If this workflow is unreliable, do not proceed to UI.

This phase proves:

```text
Rust core
SQLite
FSRS
selection
D-Bus
CLI
```

as one functioning system.

---

# Phase 11 — Build the minimal GNOME extension

Use plain modern GJS.

Structure:

```text
extension/
├── extension.js
├── panel.js
├── popup.js
├── backend.js
├── metadata.json
└── stylesheet.css
```

Initially implement only:

```text
Activities      ephemeral      Wi-Fi Battery
```

Hard requirement:

> Every D-Bus call from GNOME Shell is asynchronous.

Never use synchronous proxy calls.

`backend.js` should contain essentially all D-Bus plumbing.

---

# Phase 12 — Implement panel rotation

Extension preference:

```text
15 minutes
```

Initial options:

```text
5
10
15
30
60 minutes
```

When timer fires:

```text
popup open?
    │
   yes → set pendingRotation=true
    │
    no → await AdvanceDisplay()
```

When popup closes:

```text
pendingRotation?
    │
   yes → advance once
```

Never change the visible word while the user is reading its popup.

---

# Phase 13 — Implement the word popup

Clicking the panel word should immediately reveal the meaning.

No extra “show definition” click.

```text
┌──────────────────────────────────┐
│ ephemeral                    🔊  │
│ adjective                         │
│                                   │
│ lasting for a very short time     │
│                                   │
│ The opportunity was ephemeral.    │
│                                   │
│ [ Again ]  [ Know ]  [ Learned ] │
│                                   │
│ Next word                     →   │
├───────────────────────────────────┤
│ + Add word                    ⋮   │
└───────────────────────────────────┘
```

The panel itself is the recall prompt.

The popup is the answer.

---

# Phase 14 — Connect the three learning actions

## Again

```text
await RateAgain(id)
```

Then advance.

## Know

```text
await RateKnow(id)
```

Then advance.

## Learned

```text
await MarkLearned(id)
```

Then advance.

Show a temporary reversible confirmation:

```text
Marked “ephemeral” as learned     Undo
```

`Learned` should mean:

> Stop resurfacing this word normally.

Remove the old preference for occasionally showing learned words.

If the user wants long-term occasional repetition, they should keep pressing `Know` and let FSRS increase the interval.

---

# Phase 15 — Implement Quick Capture

Shortcut:

```text
Super+Shift+V
```

Capture flow:

```text
shortcut
   ↓
attempt PRIMARY selection
   ↓
usable vocabulary term?
   │
 yes ───────→ AddWord()
   │
 no
   ↓
inspect clipboard
```

## Clipboard behavior

Do not blindly auto-save clipboard fallback.

If PRIMARY succeeds:

```text
save immediately
```

If only clipboard has a plausible term:

```text
show tiny prefilled Add popup
```

Example:

```text
┌────────────────────────────┐
│ Add word                   │
│                            │
│ ephemeral                  │
│                            │
│ Cancel               Add   │
└────────────────────────────┘
```

This prevents stale clipboard data from being silently added.

---

# Phase 16 — Capture confirmation and undo

Successful direct capture:

```text
✓ Added “ephemeral”       Undo
```

Do not show the full definition.

The user's goal at that moment is probably to continue reading.

Principle:

```text
save now
learn later
```

Undo removes the newly added word.

Duplicate:

```text
“ephemeral” is already in your vocabulary.
```

Do not create a second entry.

---

# Phase 17 — Add dictionary enrichment

Define a Rust abstraction:

```rust
trait DictionaryProvider {
    async fn lookup(&self, term: &str) -> Result<DictionaryEntry>;
}
```

Desired result:

```text
definition
part of speech
phonetic
one example sentence
optional pronunciation metadata
```

Capture operation:

```text
AddWord()
    ↓
save immediately
    ↓
return success
    ↓
lookup asynchronously
    ↓
update word
    ↓
emit WordUpdated
```

Network failure must not make capture fail.

---

# Phase 18 — Handle incomplete words gracefully

If enrichment fails:

```text
ephemeral

Definition unavailable
```

Allow:

```text
Edit
```

or:

```text
Add definition
```

The rotation, FSRS, and vocabulary database should remain fully functional offline.

---

# Phase 19 — Add pronunciation

Only after the core workflow works.

Initial UI:

```text
ephemeral     🔊
```

Prefer the simplest reliable Linux approach.

Do not make downloaded dictionary audio a hard v1 dependency.

If pronunciation significantly complicates packaging or networking, push it to v1.1.

---

# Phase 20 — Build the GTK/libadwaita manager

Now build `wordbar-app`.

Primary view:

```text
┌──────────────────────────────────────┐
│ Wordbar                              │
│                                      │
│ Vocabulary       Preferences         │
│                                      │
│ Search vocabulary…                   │
│                                      │
│ ephemeral                        ⋮    │
│ lasting for a very short time        │
│                                      │
│ ubiquitous                       ⋮    │
│ present or found everywhere          │
│                                      │
│ 142 active · 38 learned              │
└──────────────────────────────────────┘
```

Required:

* search
* edit word
* edit definition
* edit example
* delete
* mark learned
* restore learned

No decks.

No tags.

No learning dashboard.

---

# Phase 21 — Preferences

Only expose things users actually need.

```text
Show word in top bar        on

Change word every
15 minutes

Capture shortcut
Super+Shift+V
```

And data actions:

```text
Import vocabulary…
Export vocabulary…
```

Do not expose:

* desired FSRS retention
* stability
* difficulty
* fuzzing
* scheduler parameters

---

# Phase 22 — Import/export

## Export

Portable vocabulary only.

Example:

```json
[
  {
    "term": "ephemeral",
    "definition": "lasting for a very short time",
    "example": "The opportunity was ephemeral.",
    "part_of_speech": "adjective"
  }
]
```

No scheduler internals.

Support:

```text
JSON
CSV
```

## Import

Accept progressively richer formats:

```text
word
```

or:

```text
word,definition
```

or:

```text
word,definition,example,part_of_speech
```

Missing data can be enriched later.

---

# Phase 23 — Defer exact backup/restore

Keep this concept separate from Export.

### Export

```text
my vocabulary
```

### Backup

```text
my entire Wordbar installation state
```

Backup eventually contains:

```text
words
FSRS cards
review history
statuses
settings
```

Design storage so this remains possible, but defer the dedicated Backup UI to v2 unless implementation is trivial.

---

# Phase 24 — Waybar documentation

Because `wordbarctl` already exists, document a simple Waybar integration.

Conceptually:

```text
exec:
wordbarctl current --waybar
```

Click:

```text
wordbarctl next
```

This gives Wordbar a second Linux frontend without implementing a second graphical application.

Do not make Waybar support block the GNOME release.

---

# Phase 25 — Error handling

Define expected behavior explicitly.

## Backend unavailable

Panel:

```text
—
```

Retry asynchronously.

Never freeze Shell.

## Dictionary unavailable

Save word anyway.

## Empty vocabulary

Panel:

```text
+ word
```

Popup:

```text
Select a word and press Super+Shift+V
```

## Duplicate

Do not save twice.

## Long clipboard text

Open editable Add dialog.

## Database error

Show concise error in GTK/CLI; Shell should degrade gracefully.

---

# Phase 26 — Performance constraints

GNOME extension should normally contain:

```text
one panel label
one timer
one popup
one D-Bus proxy
```

No networking.

No database access.

No subprocess polling.

Rust service should remain idle nearly all the time.

Dictionary requests happen only when needed.

---

# Phase 27 — Test matrix

Before release, test four layers independently.

## Rust unit tests

```text
normalization
duplicates
SQLite
FSRS mapping
learned/restore
selector
import/export
```

## D-Bus integration tests

```text
activation
AddWord
current
advance
Again
Know
Learned
signals
```

## CLI tests

```text
plain output
JSON output
Waybar output
errors
exit codes
```

## GNOME manual/integration tests

```text
panel appears
rotation works
popup freezes rotation
Again works
Know works
Learned works
shortcut works
clipboard fallback works
backend restart doesn't freeze Shell
extension disable cleans up everything
```

---

# Phase 28 — Primary acceptance test

The entire product is ready when this feels seamless:

```text
User reads:

"The effects were ephemeral."
```

They select:

```text
ephemeral
```

and press:

```text
Super+Shift+V
```

Wordbar shows:

```text
✓ Added “ephemeral”       Undo
```

They continue working.

Later the top bar shows:

```text
ephemeral
```

They try to remember the meaning.

They click it:

```text
ephemeral
adjective

lasting for a very short time

The opportunity was ephemeral.

[ Again ]   [ Know ]   [ Learned ]
```

They press:

```text
Again
```

FSRS records failure.

The popup advances.

Later `ephemeral` resurfaces.

They press:

```text
Know
```

FSRS increases its interval.

Eventually:

```text
Learned
```

removes it from normal rotation.

That is the complete core product.

---

# Recommended implementation order

The practical order I would follow is:

1. **Rust workspace**
2. **domain models**
3. **normalization**
4. **SQLite**
5. **FSRS integration**
6. **display selector**
7. **D-Bus service**
8. **D-Bus activation**
9. **`wordbarctl`**
10. **backend end-to-end tests**
11. **minimal GNOME panel**
12. **async D-Bus integration**
13. **panel rotation**
14. **popup**
15. **Again / Know / Learned**
16. **PRIMARY-selection capture**
17. **clipboard fallback**
18. **capture confirmation + Undo**
19. **dictionary enrichment**
20. **offline/error handling**
21. **GTK vocabulary manager**
22. **preferences**
23. **import/export**
24. **pronunciation if still simple**
25. **Waybar documentation**
26. **packaging**
27. **release testing**

The key milestone is **step 10**: once `wordbarctl` can drive the full Wordbar workflow reliably, everything after that is primarily frontend work.
