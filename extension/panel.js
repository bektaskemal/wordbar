/* panel.js — the top bar indicator and its popup menu.
 *
 * Contains exactly one label, one popup and no state of its own beyond the
 * word being shown; rotation, capture and D-Bus live in extension.js and
 * backend.js.
 */

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

import {WordCard} from './popup.js';
import {ellipsize} from './terms.js';

/** Shown while the backend is unreachable (Phase 25). */
const UNAVAILABLE_TEXT = '—';

/**
 * Shown when the vocabulary is empty.
 *
 * Resolved lazily on purpose: gettext identifies the calling extension from
 * the stack, so it throws while this module is still being evaluated.
 */
function emptyText() {
    return _('+ word');
}

export const WordbarIndicator = GObject.registerClass({
    GTypeName: 'WordbarIndicator',
    Signals: {
        'rated-again': {param_types: [GObject.TYPE_INT64]},
        'rated-know': {param_types: [GObject.TYPE_INT64]},
        'marked-learned': {param_types: [GObject.TYPE_INT64]},
        'add-requested': {},
        'vocabulary-requested': {},
        'preferences-requested': {},
        'speak-requested': {param_types: [GObject.TYPE_INT64]},
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

        this._label = new St.Label({
            style_class: 'wordbar-panel-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this.add_child(this._label);
        this._renderLabel();

        this._buildMenu();

        this.menu.connect('open-state-changed', (menu, isOpen) => {
            this.emit(isOpen ? 'popup-opened' : 'popup-closed');
        });
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
        this._card.connect('marked-learned', () => this._emitForWord('marked-learned'));
        this._card.connect('speak-requested', () => this._emitForWord('speak-requested'));
        this._card.connect('edit-requested', () => this._emitForWord('edit-requested'));

        // No "Next word" item on purpose: all three ratings advance, and a real
        // PopupMenuItem here would take the Shell's automatic selection
        // highlight, making the least important action look like the primary
        // one.
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        const footerBox = new St.BoxLayout({
            style_class: 'wordbar-footer',
            x_expand: true,
        });

        const addButton = new St.Button({
            style_class: 'wordbar-footer-button',
            label: _('+ Add word'),
            x_expand: true,
            x_align: Clutter.ActorAlign.START,
            can_focus: true,
        });
        addButton.connect('clicked', () => {
            this.menu.close();
            this.emit('add-requested');
        });
        footerBox.add_child(addButton);

        const moreButton = new St.Button({
            style_class: 'wordbar-icon-button',
            child: new St.Icon({icon_name: 'view-more-symbolic', icon_size: 16}),
            y_align: Clutter.ActorAlign.CENTER,
            can_focus: true,
        });
        moreButton.connect('clicked', () => this._toggleMoreMenu());
        footerBox.add_child(moreButton);
        this.menu.box.add_child(footerBox);

        // Secondary actions live behind "⋮" so the primary card stays clean.
        this._moreSection = new PopupMenu.PopupMenuSection();
        this._moreSection.actor.visible = false;

        const vocabularyItem = new PopupMenu.PopupMenuItem(_('Vocabulary…'));
        vocabularyItem.connect('activate', () => {
            this.menu.close();
            this.emit('vocabulary-requested');
        });
        this._moreSection.addMenuItem(vocabularyItem);

        const preferencesItem = new PopupMenu.PopupMenuItem(_('Preferences…'));
        preferencesItem.connect('activate', () => {
            this.menu.close();
            this.emit('preferences-requested');
        });
        this._moreSection.addMenuItem(preferencesItem);

        this.menu.addMenuItem(this._moreSection);
    }

    _toggleMoreMenu() {
        this._moreSection.actor.visible = !this._moreSection.actor.visible;
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
     * Backend availability. An unreachable service shows a dash rather than
     * an error dialog: the Shell degrades, it never nags (Phase 25).
     */
    setAvailable(available) {
        this._available = available;
        this._renderLabel();
    }

    _renderLabel() {
        if (!this._available) {
            this._label.text = UNAVAILABLE_TEXT;
            return;
        }
        if (!this._word) {
            this._label.text = emptyText();
            return;
        }
        this._label.text = ellipsize(this._word.term, this._maxChars);
    }
});
