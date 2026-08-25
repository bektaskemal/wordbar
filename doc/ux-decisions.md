# UX decisions

Decided deliberately, with the reasoning, so they are not re-argued from
memory. Where a decision reverses an earlier one, the reason is recorded --
several early choices were made while a bug made the alternative look broken.

## The review loop

**An empty vocabulary shows no rating buttons**, but does offer "+ Add word".
There is nothing to rate, and a greyed-out pair still reads as something you
failed to use -- while an empty card is exactly where a newcomer is looking, so
it has to offer the way in. Once there are words, adding lives in the
right-click menu.

**Two ratings: Again and Know.** Not the full FSRS four. A glanceable panel is
not a flashcard app, and a judgement call between Hard and Good on every word
costs more than the scheduling accuracy is worth.

**Learned is not a popup button.** Archiving is housekeeping, done while
reviewing the list, not mid-glance. It lives in the manager only.

**Learning and Known are views over the schedule, not stored states.** A word
crosses into Known once its review interval passes two months, which is long
enough that recalling it again is real evidence rather than short-term memory.
Deriving it means no bookkeeping and no way for the split to fall out of step
with the scheduler -- marking words by hand is the kind of chore that never
gets done, so the list would have lied.

**A known word stays in the rotation.** Knowing a word is not a reason to stop
testing it; FSRS simply shows it rarely, which is the whole point of a growing
interval. Only an explicit archive takes a word out, and an archived word
counts as known too.

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

**The dot means "a rotation passed you by"**: it appears once a word has waited
a full interval unnoticed, and clears when its popup opens. Two earlier
definitions were worse. The FSRS due date carried no information, because the
selector already prefers due words, so it was lit almost always -- and lit
permanently in a fresh dev database where every card is due. Marking a word
unseen from the moment it arrived was better but still lit most of the time,
since most words are never clicked. Waiting for a missed interval means the dot
is off while a word is fresh and only lights when the reader actually missed
something, which is the question they have: did I miss one?

**Display is never a review.** Only an explicit Again or Know touches FSRS.

## Top bar

**`Wordbar` when the vocabulary is empty**, with the popup explaining capture.
A named, clickable target beats a cryptic `+ word`.

**A warning icon when the backend is unreachable**, never a frozen or blank
panel. The Shell degrades; it never nags and never blocks.

**A dot after the word when it is due.** Explicit and readable at a glance,
unlike a brightness or weight difference.

**Right-click opens a context menu**: Add word, Vocabulary, Preferences, Quit.
Adding is a destination too, so it sits there rather than in the card. A second
menu rather than entries in the card, because the card is the product and
these are destinations, not part of reviewing a word. Accepted cost:
undiscoverable, since nothing in GNOME advertises a right-click action on a
panel item -- mitigated by a pointer in the empty-state popup, which is where a
new user looks, and by the menu being the only place these are named.

**Quit puts Wordbar away until the next login.** It stops the service and
disables the extension, so the indicator goes rather than sitting there in a
warning state -- which was a confusing half-measure when the user had asked for
it to be gone. Disabling does not touch the enabled-extensions setting, so it
returns at the next login or whenever it is re-enabled, and the menu item says
so.

Asking a stopped service to stop is also a no-op rather than a paradox: a D-Bus
call activates the service to deliver it, so an unguarded Quit would start
Wordbar just to tell it to stop.

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

**Switching sense sticks**, confirmed with a brief "New default" -- the new
sense is already visible in the card, so the toast only has to say that it
stuck, not repeat what changed.

**A reset returns to the sense the card opened with.** It appears only once the
selection has moved, so there is always something to go back to, and it makes
stepping through senses safe to explore: committing on every step would
otherwise be a trap. It sits to the left of the arrows and keeps its space
whether or not it applies -- appearing between them would slide them under the
pointer that just clicked one, so the next step would land on reset instead.

**The "needs a definition" view announces itself.** Opening the manager from
the popup's hint filters the list, and that filter shows a banner with a way
out. It was first an invisible one-way switch: once every definition was
filled in it silently filtered everything away, and the manager simply said
there were no words.

**Words with no definition are collected, not prompted for.** The popup footer
carries a quiet count that opens the manager filtered to them, so they are
fixed in one sitting. Prompting at capture time would interrupt exactly when
the user wanted to keep reading -- and offline, every capture would interrupt.

**Enrichment can never fail a capture.** It runs after the word is stored, and
imported words are looked up the same way -- sequentially and paced, since an
import can be hundreds of words and a free dictionary API deserves better than
a burst.

**A lookup never overwrites an example you brought.** The sentence you met a
word in, imported from a Kindle or typed by hand, is irreplaceable; a
dictionary's example is not. A definition you supplied survives too, unless you
ask for a re-lookup explicitly.

## Pronunciation

**The dictionary's recording, falling back to TTS.** A real human voice where
one exists, synthesised otherwise. Playback happens in the service, not the
Shell, so the compositor never runs a subprocess.

## Manager

**All preferences live here**, the capture shortcut included. One window, so
there is never a question of which one to open. It writes the extension's
GSettings, so the panel still applies changes immediately.

**No "show word in top bar" switch.** Turning it off left an extension that
did nothing but host a keyboard shortcut -- the panel word is the product. Not
wanting it means disabling the extension, which is where GNOME expects that
control anyway.

**Import and export sit with the vocabulary**, in the window's menu, not among
the preferences: they act on the words, not on how the panel behaves. The note
about scheduler internals went with them -- what an export omits is
documented, not something to explain in the UI every time.

**Choosing a sense is a row action**, not only a field in the editor. A wrong
sense is common enough to deserve a one-step route rather than being buried in
a form.

**Row actions stay behind a per-row menu.** Compact, and it scales as actions
are added.

**Deleting asks first**, because it discards review history too. Archiving does
not, because it is reversible and offers an undo.
