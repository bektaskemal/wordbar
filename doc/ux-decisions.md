# UX decisions

Decided deliberately, with the reasoning, so they are not re-argued from
memory. Where a decision reverses an earlier one, the reason is recorded --
several early choices were made while a bug made the alternative look broken.

## The review loop

**Two ratings: Again and Know.** Not the full FSRS four. A glanceable panel is
not a flashcard app, and a judgement call between Hard and Good on every word
costs more than the scheduling accuracy is worth.

**Learned is not a popup button.** Archiving is housekeeping, done while
reviewing the list, not mid-glance. It lives in the manager only.

**Rating closes the popup and advances.** The next word belongs in the top bar
as a prompt; showing its card would spend the recall test before it is asked.
This was first built as "stay open", chosen only because a bug made the
advance invisible -- rating reverted to the word just rated. With that fixed,
closing is what the learning model wants.

**Rotation every 15 minutes**, options 5/10/15/30/60, but the timer paces the
reader rather than itself: a word that has not been looked at holds the panel
instead of being rotated past. After two held intervals the panel gives up and
moves on, so ignoring Wordbar can never freeze it on one word. At the default
interval a word you ignore occupies the panel for at most three intervals.

**The dot means "you have not looked at this one yet"**, and clears when its
popup opens. It was first implemented as the FSRS due date, which turned out to
carry no information: the selector already prefers due words, so a due marker
is lit almost always. Unseen-since-it-appeared is a signal that actually turns
off, and it answers a question the reader has -- did I miss one? -- rather than
one the scheduler already handles.

**Display is never a review.** Only an explicit Again or Know touches FSRS.

## Top bar

**`Wordbar` when the vocabulary is empty**, with the popup explaining capture.
A named, clickable target beats a cryptic `+ word`.

**A warning icon when the backend is unreachable**, never a frozen or blank
panel. The Shell degrades; it never nags and never blocks.

**A dot after the word when it is due.** Explicit and readable at a glance,
unlike a brightness or weight difference.

**Right-click opens the manager.** Accepted cost: undiscoverable, since nothing
in GNOME advertises a right-click action on a panel item. Mitigated by a
pointer in the empty-state popup, which is where a new user looks.

## Capture

**Save immediately, confirm with an undoable toast.** Save now, learn later:
the user's goal at that moment is to keep reading. The toast sits under the top
bar near the word, connecting the confirmation to where the word will appear.

**A long selection is refused, not silently mangled.** It reports "Select a word
or short phrase" and saves nothing. The previous behaviour discarded the
selection and opened an empty box -- or worse, prefilled it from an unrelated
clipboard.

**A duplicate is reported and nothing else happens.** No second entry, no
hijacking the panel.

## Definitions

**Every sense is stored**, ranked so the commonest part of speech comes first,
and switchable from both the popup and the manager. A single stored sense
picked the noun reading of "ephemeral", which is not what anyone means by it.

**Switching sense sticks.** Switching is how a wrong pick gets corrected, so it
becomes that word's sense and the panel uses it from then on, confirmed briefly
so the persistence is not a surprise.

**Words with no definition are collected, not prompted for.** The popup footer
carries a quiet count that opens the manager filtered to them, so they are
fixed in one sitting. Prompting at capture time would interrupt exactly when
the user wanted to keep reading -- and offline, every capture would interrupt.

**Enrichment can never fail a capture.** It runs after the word is stored.

## Pronunciation

**The dictionary's recording, falling back to TTS.** A real human voice where
one exists, synthesised otherwise. Playback happens in the service, not the
Shell, so the compositor never runs a subprocess.

## Manager

**All preferences live here**, the capture shortcut included. One window, so
there is never a question of which one to open. It writes the extension's
GSettings, so the panel still applies changes immediately.

**Row actions stay behind a per-row menu.** Compact, and it scales as actions
are added.

**Deleting asks first**, because it discards review history too. Archiving does
not, because it is reversible and offers an undo.
