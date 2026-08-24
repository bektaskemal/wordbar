/* extension.js — wiring.
 *
 * Holds exactly what Phase 26 allows: one panel label, one timer, one popup
 * and one D-Bus proxy. No networking, no database, no subprocess polling.
 */

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension, gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Backend, ERROR_DUPLICATE, errorName} from './backend.js';
import {WordbarIndicator} from './panel.js';
import {capture, showAddDialog} from './capture.js';
import {dismissToast, showToast, showUndoToast} from './toast.js';

const CAPTURE_SHORTCUT_KEY = 'capture-shortcut';

export default class WordbarExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._pendingRotation = false;
        this._rotationSource = 0;
        this._settingsIds = [];

        this._backend = new Backend();
        this._indicator = new WordbarIndicator(this._settings.get_int('max-panel-chars'));

        this._connectIndicator();
        this._connectBackend();
        this._connectSettings();

        Main.panel.addToStatusArea(this.uuid, this._indicator, 0, 'right');
        this._applyVisibility();
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
        indicator.connect('marked-learned', (_i, id) => this._markLearned(id));
        indicator.connect('next-requested', () => this._advance());
        indicator.connect('add-requested', () => this._showAddDialog(''));
        indicator.connect('vocabulary-requested', () => this._openManager('openVocabulary'));
        indicator.connect('preferences-requested', () => this._openManager('openPreferences'));
        indicator.connect('edit-requested', () => this._openManager('openVocabulary'));
        indicator.connect('speak-requested', (_i, id) => this._speak(id));

        // Phase 12: never change the visible word while its popup is open.
        indicator.connect('popup-closed', () => {
            if (this._pendingRotation) {
                this._pendingRotation = false;
                this._advance();
            }
        });
    }

    _connectBackend() {
        this._backend.connect('current-word-changed', (_b, word) => {
            this._indicator?.setWord(word.term ? word : null);
        });

        // Enrichment landed: refresh the card if it is the word on screen.
        this._backend.connect('word-updated', (_b, id) => {
            const current = this._indicator?.currentWord;
            if (current && current.id === id)
                this._refreshDetails(id);
        });

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
        this._settingsIds.push(this._settings.connect('changed::show-in-top-bar', () => {
            this._applyVisibility();
        }));
        this._settingsIds.push(this._settings.connect('changed::max-panel-chars', () => {
            this._indicator?.setMaxChars(this._settings.get_int('max-panel-chars'));
        }));
        this._settingsIds.push(this._settings.connect(`changed::${CAPTURE_SHORTCUT_KEY}`, () => {
            this._removeKeybinding();
            this._addKeybinding();
        }));
    }

    /**
     * Hide both the button and its panel container, so a hidden Wordbar leaves
     * no gap in the top bar. The indicator itself stays alive: destroying it
     * belongs to disable().
     */
    _applyVisibility() {
        const show = this._settings.get_boolean('show-in-top-bar');
        this._indicator.visible = show;
        this._indicator.container.visible = show;
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
        this._advance();
    }

    // -- backend calls ----------------------------------------------------

    async _refresh() {
        try {
            const word = await this._backend.getCurrentWord();
            this._indicator?.setWord(word);
        } catch (error) {
            this._logFailure('reading the current word', error);
        }
    }

    async _refreshDetails(id) {
        try {
            const word = await this._backend.getWordDetails(id);
            if (word)
                this._indicator?.setWord(word);
        } catch (error) {
            this._logFailure('refreshing word details', error);
        }
    }

    async _advance() {
        try {
            const word = await this._backend.advanceDisplay();
            this._indicator?.setWord(word);
        } catch (error) {
            this._logFailure('advancing the display', error);
        }
    }

    async _rate(id, method) {
        this._indicator?.menu.close();
        try {
            const next = await this._backend[method](id);
            this._indicator?.setWord(next);
        } catch (error) {
            this._logFailure('recording the rating', error);
        }
    }

    async _markLearned(id) {
        const word = this._indicator?.currentWord;
        const term = word?.term ?? '';
        this._indicator?.menu.close();

        try {
            const next = await this._backend.markLearned(id);
            this._indicator?.setWord(next);
            showUndoToast(
                _('Marked “%s” as learned').replace('%s', term),
                () => this._restore(id)
            );
        } catch (error) {
            this._logFailure('marking the word learned', error);
        }
    }

    async _restore(id) {
        try {
            await this._backend.restoreWord(id);
        } catch (error) {
            this._logFailure('restoring the word', error);
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
        capture({onCapture: text => this._addWord(text)}).catch(error => {
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

    async _openManager(method) {
        try {
            await this._backend[method]();
        } catch (error) {
            showToast(_('Could not open the Wordbar manager.'));
            this._logFailure('opening the manager', error);
        }
    }

    /**
     * Pronunciation, Phase 19: hand the term to the session's speech service
     * if one is installed. Deliberately not a hard dependency — a missing
     * spd-say is a no-op, not an error dialog.
     */
    _speak(id) {
        const word = this._indicator?.currentWord;
        if (!word || word.id !== id)
            return;
        try {
            const [ok, argv] = GLib.shell_parse_argv(`spd-say -- ${GLib.shell_quote(word.term)}`);
            if (!ok)
                return;
            Gio.Subprocess.new(argv, Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE);
        } catch (_error) {
            // No speech dispatcher installed; pronunciation is optional.
        }
    }

    _describe(error) {
        const message = error?.message ?? String(error);
        // Strip the "GDBus.Error:org.wordbar.Error.X:" prefix users should not see.
        return message.replace(/^GDBus\.Error:[^:]+:\s*/, '');
    }

    _logFailure(what, error) {
        console.debug(`Wordbar: ${what} failed: ${error?.message ?? error}`);
    }
}
