/* panel.js — the top bar indicator and its popup menu.
 *
 * Contains exactly one label, one popup and no state of its own beyond the
 * word being shown; rotation, capture and D-Bus live in extension.js and
 * backend.js.
 */

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

import {WordCard} from './popup.js';
import {ellipsize} from './terms.js';

/** Marks a word that has waited a whole interval without being looked at. */
const WAITING_MARK = ' \u2022';

/**
 * Shown when the vocabulary is empty: a name to click, not a cryptic mark.
 *
 * Resolved lazily on purpose: gettext identifies the calling extension from
 * the stack, so it throws while this module is still being evaluated.
 */
function emptyText() {
    return _('Wordbar');
}

export const WordbarIndicator = GObject.registerClass({
    GTypeName: 'WordbarIndicator',
    Signals: {
        'rated-again': {param_types: [GObject.TYPE_INT64]},
        'rated-know': {param_types: [GObject.TYPE_INT64]},
        'add-requested': {},
        'vocabulary-requested': {},
        'preferences-requested': {},
        'quit-requested': {},
        'incomplete-requested': {},
        'speak-requested': {param_types: [GObject.TYPE_INT64]},
        'sense-selected': {param_types: [GObject.TYPE_INT64]},
        'sense-reset': {},
        'edit-requested': {param_types: [GObject.TYPE_INT64]},
        'popup-opened': {},
        'popup-closed': {},
    },
}, class WordbarIndicator extends PanelMenu.Button {
    _init(maxChars) {
        super._init(0.0, 'Wordbar', false);

        this._maxChars = maxChars;
        this._word = null;
        this._available = true;
        this._waiting = false;

        const box = new St.BoxLayout({style_class: 'wordbar-panel-box'});
        this._warning = new St.Icon({
            style_class: 'wordbar-panel-warning',
            icon_name: 'dialog-warning-symbolic',
            icon_size: 16,
            visible: false,
        });
        box.add_child(this._warning);
        this._label = new St.Label({
            style_class: 'wordbar-panel-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        box.add_child(this._label);
        this.add_child(box);
        this._renderLabel();

        this._buildContextMenu();

        // Right-click opens the context menu. Undiscoverable by nature, so the
        // empty-state popup spells it out -- and the menu is the only place
        // these destinations are named.
        this.connect('button-press-event', (_actor, event) => {
            if (event.get_button() === Clutter.BUTTON_SECONDARY) {
                this.menu.close();
                this._contextMenu.toggle();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });

        this._buildMenu();

        this.menu.connect('open-state-changed', (menu, isOpen) => {
            this.emit(isOpen ? 'popup-opened' : 'popup-closed');
        });
    }

    /**
     * The right-click menu.
     *
     * A second menu rather than entries in the main popup: the card is the
     * product, and these are destinations, not part of reviewing a word.
     */
    _buildContextMenu() {
        this._contextMenu = new PopupMenu.PopupMenu(this, 0.5, St.Side.TOP);
        this._contextMenu.actor.add_style_class_name('wordbar-context-menu');
        Main.uiGroup.add_child(this._contextMenu.actor);
        this._contextMenu.actor.hide();
        Main.panel.menuManager.addMenu(this._contextMenu);

        const add = (label, signal) => {
            const item = new PopupMenu.PopupMenuItem(label);
            item.connect('activate', () => {
                this._contextMenu.close();
                this.emit(signal);
            });
            this._contextMenu.addMenuItem(item);
        };

        add(_('Add word…'), 'add-requested');
        this._contextMenu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        add(_('Vocabulary…'), 'vocabulary-requested');
        add(_('Preferences…'), 'preferences-requested');
        this._contextMenu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        add(_('Quit Wordbar'), 'quit-requested');
    }

    _buildMenu() {
        // Added straight to the menu box, not wrapped in a PopupBaseMenuItem.
        // A non-reactive menu item carries .popup-inactive-menu-item and, being
        // non-reactive, also :insensitive -- which the Shell theme paints
        // #9a9a9a, dimming every label in the card. Plain children inherit the
        // normal #ffffff popup text colour, matching the top bar.
        this._card = new WordCard();
        this.menu.box.add_child(this._card);

        this._card.connect('rated-again', () => this._emitForWord('rated-again'));
        this._card.connect('rated-know', () => this._emitForWord('rated-know'));
        this._card.connect('speak-requested', () => this._emitForWord('speak-requested'));
        this._card.connect('sense-selected', (_c, id) => this.emit('sense-selected', id));
        this._card.connect('sense-reset', () => this.emit('sense-reset'));
        this._card.connect('edit-requested', () => this._emitForWord('edit-requested'));

        // No "Next word" item on purpose: all three ratings advance, and a real
        // PopupMenuItem here would take the Shell's automatic selection
        // highlight, making the least important action look like the primary
        // one.
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        // A quiet count of words still missing a definition, opening the
        // manager filtered to them. Fixing them in one sitting beats being
        // interrupted at capture time, and offline that would be every capture.
        this._incomplete = new St.Button({
            style_class: 'wordbar-footer-button wordbar-incomplete',
            x_expand: true,
            x_align: Clutter.ActorAlign.START,
            can_focus: true,
            visible: false,
        });
        this._incomplete.connect('clicked', () => {
            this.menu.close();
            this.emit('incomplete-requested');
        });
        this.menu.box.add_child(this._incomplete);

        // No "+ Add word" here: adding is a destination, not part of
        // reviewing the word on screen, so it lives in the right-click menu
        // beside the other destinations.
    }

    _emitForWord(signal) {
        if (this._word)
            this.emit(signal, this._word.id);
    }

    get currentWord() {
        return this._word;
    }

    get isMenuOpen() {
        return this.menu.isOpen;
    }

    setMaxChars(maxChars) {
        this._maxChars = maxChars;
        this._renderLabel();
    }

    /** Update the panel and the popup together. */
    setWord(word) {
        this._word = word && word.term ? word : null;
        this._renderLabel();
        this._card.setWord(this._word);
    }

    /**
     * Backend availability. An unreachable service shows a warning icon
     * rather than an error dialog: the Shell degrades, it never nags.
     */
    setAvailable(available) {
        this._available = available;
        this._renderLabel();
    }

    /**
     * Whether this word has waited a full rotation interval unnoticed.
     *
     * Not "unseen since it appeared", which would mark every freshly rotated
     * word and so be lit most of the time, and not the FSRS due date, which
     * the selector already prefers and so would be lit almost always. This
     * lights only when a rotation actually passed you by.
     */
    setWaiting(waiting) {
        this._waiting = waiting;
        this._renderLabel();
    }

    /**
     * Offer the current word's alternative senses in the card.
     *
     * `baselineId` is the sense the card opened with; the reset button appears
     * only once the selection has moved away from it.
     */
    setSenses(senses, baselineId = null) {
        this._card.setSenses(senses, baselineId);
    }

    /** Show how many words still need a definition, or hide the row. */
    setIncompleteCount(count) {
        this._incomplete.visible = count > 0;
        if (count > 0) {
            this._incomplete.label = count === 1
                ? _('1 word needs a definition')
                : _('%d words need definitions').replace('%d', String(count));
        }
    }

    /** PanelMenu.Button only knows about its own menu; ours must go too. */
    destroy() {
        if (this._contextMenu) {
            Main.panel.menuManager.removeMenu(this._contextMenu);
            this._contextMenu.destroy();
            this._contextMenu = null;
        }
        super.destroy();
    }

    _renderLabel() {
        this._warning.visible = !this._available;

        if (!this._available) {
            this._label.text = _('Wordbar');
            return;
        }
        if (!this._word) {
            this._label.text = emptyText();
            return;
        }

        const term = ellipsize(this._word.term, this._maxChars);
        this._label.text = this._waiting ? `${term}${WAITING_MARK}` : term;
    }
});
