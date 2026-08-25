/* extension.js — wiring.
 *
 * Holds exactly what Phase 26 allows: one panel label, one timer, one popup
 * and one D-Bus proxy. No networking, no database, no subprocess polling.
 */

import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension, gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Backend, ERROR_DUPLICATE, errorName} from './backend.js';
import {WordbarIndicator} from './panel.js';
import {capture, showAddDialog} from './capture.js';
import {dismissToast, showToast, showUndoToast} from './toast.js';

const CAPTURE_SHORTCUT_KEY = 'capture-shortcut';

/**
 * How many rotation intervals a word may hold the panel while unseen.
 *
 * Holding means a rotation never skips past a word you never looked at; the
 * cap means the panel can never sit on one word all day. At the default
 * interval a word you ignore occupies the panel for at most three intervals.
 */
const MAX_HOLDS = 2;

export default class WordbarExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._pendingRotation = false;
        this._rotationSource = 0;
        this._settingsIds = [];
        this._currentId = null;
        this._seen = false;
        this._holds = 0;
        // The sense the card opened with, so a switch can be undone.
        this._senseBaseline = null;

        this._backend = new Backend();
        this._indicator = new WordbarIndicator(this._settings.get_int('max-panel-chars'));

        this._connectIndicator();
        this._connectBackend();
        this._connectSettings();

        Main.panel.addToStatusArea(this.uuid, this._indicator, 0, 'right');
        this._addKeybinding();
        this._startRotationTimer();

        // First read also activates the service via the session bus.
        this._refresh();
    }

    disable() {
        // Everything created in enable() is undone here: an extension that
        // leaks a timer or a keybinding breaks the whole Shell session.
        this._stopRotationTimer();
        this._removeKeybinding();
        dismissToast();

        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];

        this._backend?.destroy();
        this._backend = null;

        this._indicator?.destroy();
        this._indicator = null;

        this._settings = null;
        this._pendingRotation = false;
    }

    // -- wiring -----------------------------------------------------------

    _connectIndicator() {
        const indicator = this._indicator;

        indicator.connect('rated-again', (_i, id) => this._rate(id, 'rateAgain'));
        indicator.connect('rated-know', (_i, id) => this._rate(id, 'rateKnow'));
        indicator.connect('add-requested', () => this._showAddDialog(''));
        indicator.connect('vocabulary-requested', () => this._openManager('openVocabulary'));
        indicator.connect('preferences-requested', () => this._openManager('openPreferences'));
        indicator.connect('quit-requested', () => this._quitService());
        indicator.connect('incomplete-requested', () => this._openManager('openIncomplete'));
        // "Add definition" opens that word's editor, not the whole list.
        indicator.connect('edit-requested', (_i, id) => this._editWord(id));
        indicator.connect('speak-requested', (_i, id) => this._speak(id));
        indicator.connect('sense-selected', (_i, senseId) => this._selectSense(senseId));
        indicator.connect('sense-reset', () => this._resetSense());

        // Opening the popup is what "seen" means: you have been given the
        // chance to recall this word, so the panel may move on.
        indicator.connect('popup-opened', () => this._markSeen());

        // Never change the visible word while its popup is open.
        indicator.connect('popup-closed', () => {
            if (this._pendingRotation) {
                this._pendingRotation = false;
                this._advance();
            }
        });
    }

    _connectBackend() {
        this._backend.connect('current-word-changed', (_b, word) => {
            this._showWord(word.term ? word : null);
        });

        // Enrichment landed: refresh the card if it is the word on screen.
        this._backend.connect('word-updated', (_b, id) => {
            const current = this._indicator?.currentWord;
            if (current && current.id === id)
                this._refreshDetails(id);
        });

        this._backend.connect('vocabulary-changed', () => this._refreshIncomplete());

        this._backend.connect('availability-changed', (_b, available) => {
            this._indicator?.setAvailable(available);
            if (available)
                this._refresh();
        });
    }

    _connectSettings() {
        this._settingsIds.push(this._settings.connect('changed::rotation-minutes', () => {
            this._startRotationTimer();
        }));
        this._settingsIds.push(this._settings.connect('changed::max-panel-chars', () => {
            this._indicator?.setMaxChars(this._settings.get_int('max-panel-chars'));
        }));
        this._settingsIds.push(this._settings.connect(`changed::${CAPTURE_SHORTCUT_KEY}`, () => {
            this._removeKeybinding();
            this._addKeybinding();
        }));
    }

    // -- rotation ---------------------------------------------------------

    _startRotationTimer() {
        this._stopRotationTimer();
        const minutes = Math.max(1, this._settings.get_int('rotation-minutes'));
        this._rotationSource = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            minutes * 60,
            () => {
                this._onRotationTick();
                return GLib.SOURCE_CONTINUE;
            }
        );
    }

    _stopRotationTimer() {
        if (this._rotationSource) {
            GLib.Source.remove(this._rotationSource);
            this._rotationSource = 0;
        }
    }

    _onRotationTick() {
        if (this._indicator?.isMenuOpen) {
            // The user is reading this word right now; defer until they close it.
            this._pendingRotation = true;
            return;
        }

        // Hold an unseen word rather than skipping past it, but only so long:
        // otherwise ignoring the panel would freeze it on one word. The dot
        // appears here rather than when the word arrives, so it means "a
        // rotation passed you by" instead of merely "this word is new".
        if (!this._seen && this._holds < MAX_HOLDS) {
            this._holds += 1;
            this._indicator?.setWaiting(true);
            return;
        }

        this._advance();
    }

    /** Fetch the alternative senses for the word on screen. */
    async _loadSenses(id) {
        if (id === null) {
            this._senses = [];
            this._indicator?.setSenses([], null);
            return;
        }
        try {
            const senses = await this._backend.listSenses(id);
            // The panel may have moved on while we were waiting.
            if (this._indicator?.currentWord?.id !== id)
                return;

            this._senses = senses;
            // A card already open when senses arrive still counts as opened on
            // whatever they show now.
            if (this._senseBaseline === null && this._indicator?.isMenuOpen)
                this._captureSenseBaseline();
            else
                this._indicator?.setSenses(senses, this._senseBaseline);
        } catch (error) {
            this._senses = [];
            this._indicator?.setSenses([], null);
            this._logFailure('listing senses', error);
        }
    }

    /**
     * Switch the word to another of its senses.
     *
     * Confirmed briefly, because the change persists -- silently rewriting the
     * stored definition would be a surprise. The new sense is already visible
     * in the card, so the toast only has to say that it stuck.
     */
    async _selectSense(senseId) {
        await this._applySense(senseId, _('New default'));
    }

    async _applySense(senseId, message) {
        try {
            const word = await this._backend.selectSense(senseId);
            if (!word)
                return;
            this._indicator?.setWord(word);
            await this._loadSenses(word.id);
            showToast(message, null, 3);
        } catch (error) {
            this._logFailure('selecting a sense', error);
        }
    }

    /** Record that the current word has been looked at, and clear its dot. */
    _markSeen() {
        this._seen = true;
        this._holds = 0;
        this._indicator?.setWaiting(false);
        // Opening the card is what "last time" means for the reset button.
        this._captureSenseBaseline();
    }

    /**
     * Remember which sense is showing as the card opens.
     *
     * Senses can arrive after the popup is already open, for a word captured
     * moments ago, so this is called again when they load.
     */
    _captureSenseBaseline() {
        const senses = this._senses ?? [];
        const selected = senses.find(sense => sense.selected);
        this._senseBaseline = selected ? selected.id : null;
        this._indicator?.setSenses(senses, this._senseBaseline);
    }

    /** Go back to the sense the card opened with. */
    async _resetSense() {
        if (this._senseBaseline === null)
            return;
        await this._applySense(this._senseBaseline, _('Reverted'));
    }

    /**
     * Put a word on screen, resetting the seen state when it is a new one.
     *
     * Every path that changes the panel word goes through here so the dot and
     * the hold counter cannot drift out of step with what is displayed.
     */
    _showWord(word) {
        const id = word?.id ?? null;
        if (id !== this._currentId) {
            this._currentId = id;
            this._seen = false;
            this._holds = 0;
            this._senseBaseline = null;
            this._senses = [];
        }
        this._indicator?.setWord(word);
        // A newly arrived word carries no dot: it has not been missed yet.
        this._indicator?.setWaiting(this._holds > 0 && !this._seen);
        this._loadSenses(word?.id ?? null);
    }

    // -- backend calls ----------------------------------------------------

    async _refresh() {
        try {
            const word = await this._backend.getCurrentWord();
            this._showWord(word);
        } catch (error) {
            this._logFailure('reading the current word', error);
        }
        this._refreshIncomplete();
    }

    /** Count words still missing a definition, for the popup footer. */
    async _refreshIncomplete() {
        try {
            const count = await this._backend.countIncomplete();
            this._indicator?.setIncompleteCount(count);
        } catch (error) {
            this._indicator?.setIncompleteCount(0);
            this._logFailure('counting incomplete words', error);
        }
    }

    /**
     * Re-read one word, used when enrichment fills in its definition.
     *
     * The panel can advance while this request is in flight: rating a word
     * emits WordUpdated for it and only then CurrentWordChanged for its
     * replacement. So the reply is dropped unless that word is still the one
     * on screen -- otherwise a rating visibly reverts to the word just rated.
     */
    async _refreshDetails(id) {
        try {
            const word = await this._backend.getWordDetails(id);
            if (word && this._indicator?.currentWord?.id === id) {
                // Same word, enriched: keep the seen state, only refresh content.
                this._indicator.setWord(word);
                // Senses arrive with enrichment, i.e. after the word first
                // appeared, so they have to be picked up here too.
                this._loadSenses(id);
            }
        } catch (error) {
            this._logFailure('refreshing word details', error);
        }
    }

    async _advance() {
        try {
            const word = await this._backend.advanceDisplay();
            this._showWord(word);
        } catch (error) {
            this._logFailure('advancing the display', error);
        }
    }

    /**
     * Rate the current word and move to the next one.
     *
     * The popup closes on purpose: the next word belongs in the top bar as a
     * prompt, and revealing its meaning here would spend the recall test
     * before it is asked.
     */
    async _rate(id, method) {
        // The rating advances by itself, so a rotation deferred while the
        // popup was open is already satisfied. Leaving it set would make
        // closing the popup advance a second time and skip a word.
        this._pendingRotation = false;
        this._indicator?.menu.close();
        try {
            const next = await this._backend[method](id);
            this._showWord(next);
        } catch (error) {
            this._logFailure('recording the rating', error);
        }
    }

    // -- capture ----------------------------------------------------------

    _addKeybinding() {
        Main.wm.addKeybinding(
            CAPTURE_SHORTCUT_KEY,
            this._settings,
            Meta.KeyBindingFlags.NONE,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => this._onCaptureShortcut()
        );
    }

    _removeKeybinding() {
        Main.wm.removeKeybinding(CAPTURE_SHORTCUT_KEY);
    }

    _onCaptureShortcut() {
        capture({
            onCapture: text => this._addWord(text),
            onRejected: () => showToast(_('Select a word or short phrase.')),
        }).catch(error => {
            this._logFailure('capturing the selection', error);
        });
    }

    _showAddDialog(prefill) {
        showAddDialog(prefill, text => this._addWord(text));
    }

    async _addWord(text) {
        try {
            const word = await this._backend.addWord(text);
            // Phase 16: confirm the save, do not show the definition. The user
            // is going straight back to what they were reading.
            showUndoToast(
                _('✓ Added “%s”').replace('%s', word?.term ?? text),
                () => this._undoAdd(word?.id)
            );
        } catch (error) {
            if (errorName(error) === ERROR_DUPLICATE) {
                showToast(_('“%s” is already in your vocabulary.').replace('%s', text.trim()));
                return;
            }
            showToast(this._describe(error));
            this._logFailure('adding a word', error);
        }
    }

    async _undoAdd(id) {
        if (id === undefined || id === null)
            return;
        try {
            await this._backend.deleteWord(id);
        } catch (error) {
            this._logFailure('undoing the capture', error);
        }
    }

    // -- misc -------------------------------------------------------------

    async _editWord(id) {
        this._indicator?.menu.close();
        try {
            await this._backend.openWordEditor(id);
        } catch (error) {
            showToast(_('Could not open the Wordbar manager.'));
            this._logFailure('opening the word editor', error);
        }
    }

    /**
     * Stop the background service.
     *
     * The panel drops to its unavailable state, honestly: nothing is running.
     * Anything that touches Wordbar activates it again, so this frees memory
     * rather than turning it off -- disabling the extension does that.
     */
    async _quitService() {
        try {
            await this._backend.quit();
            this._indicator?.setAvailable(false);
            showToast(_('Wordbar stopped'), null, 3);
        } catch (error) {
            this._logFailure('stopping the service', error);
        }
    }

    async _openManager(method) {
        try {
            await this._backend[method]();
        } catch (error) {
            showToast(_('Could not open the Wordbar manager.'));
            this._logFailure('opening the manager', error);
        }
    }

    /**
     * Pronounce the current word.
     *
     * The service does the work -- fetching, caching and playing, or
     * synthesising when there is no recording -- so the compositor never
     * downloads a file or spawns a media player.
     */
    async _speak(id) {
        try {
            await this._backend.speak(id);
        } catch (error) {
            this._logFailure('pronouncing the word', error);
        }
    }

    _describe(error) {
        const message = error?.message ?? String(error);
        // Strip the "GDBus.Error:org.wordbar.Error.X:" prefix users should not see.
        return message.replace(/^GDBus\.Error:[^:]+:\s*/, '');
    }

    _logFailure(what, error) {
        // console.warn, not debug: warnings reach the journal, and an
        // extension failure with no visible trace is close to undebuggable.
        console.warn(`Wordbar: ${what} failed: ${error?.message ?? error}`);
    }
}
