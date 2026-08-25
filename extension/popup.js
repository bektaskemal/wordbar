/* popup.js — the answer to the panel's prompt.
 *
 * The panel word is the recall test; opening this popup reveals the meaning
 * immediately. There is deliberately no "show definition" step.
 */

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import {gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

/**
 * The word card: term, part of speech, definition, example, and the three
 * learning actions.
 */
export const WordCard = GObject.registerClass({
    GTypeName: 'WordbarWordCard',
    Signals: {
        'rated-again': {},
        'rated-know': {},
        'speak-requested': {},
        'sense-selected': {param_types: [GObject.TYPE_INT64]},
        'sense-reset': {},
        'edit-requested': {},
    },
}, class WordCard extends St.BoxLayout {
    _init() {
        super._init({
            style_class: 'wordbar-card',
            vertical: true,
            x_expand: true,
        });

        const header = new St.BoxLayout({style_class: 'wordbar-card-header', x_expand: true});
        this._term = new St.Label({style_class: 'wordbar-term', x_expand: true});
        this._term.clutter_text.line_wrap = true;
        header.add_child(this._term);

        // Pronunciation is best-effort (Phase 19): the button only appears
        // when the service actually gave us something to speak.
        this._speaker = new St.Button({
            style_class: 'wordbar-icon-button',
            child: new St.Icon({icon_name: 'audio-speakers-symbolic', icon_size: 16}),
            y_align: Clutter.ActorAlign.CENTER,
            can_focus: true,
        });
        this._speaker.connect('clicked', () => this.emit('speak-requested'));
        header.add_child(this._speaker);
        this.add_child(header);

        // Part of speech, plus a switcher when the dictionary gave more than
        // one sense. A wrong pick should be one click to fix, not a retype.
        const senseRow = new St.BoxLayout({style_class: 'wordbar-sense-row'});
        this._partOfSpeech = new St.Label({
            style_class: 'wordbar-part-of-speech',
            y_align: Clutter.ActorAlign.CENTER,
            x_expand: true,
        });
        senseRow.add_child(this._partOfSpeech);

        this._prevSense = new St.Button({
            style_class: 'wordbar-sense-step',
            label: '\u2039',
            can_focus: true,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._prevSense.connect('clicked', () => this._stepSense(-1));
        senseRow.add_child(this._prevSense);

        this._senseCount = new St.Label({
            style_class: 'wordbar-sense-count',
            y_align: Clutter.ActorAlign.CENTER,
        });
        senseRow.add_child(this._senseCount);

        this._nextSense = new St.Button({
            style_class: 'wordbar-sense-step',
            label: '\u203a',
            can_focus: true,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._nextSense.connect('clicked', () => this._stepSense(1));
        senseRow.add_child(this._nextSense);

        // Only appears once you have moved away from the sense the card
        // opened with, so there is always something to go back to.
        this._resetSense = new St.Button({
            style_class: 'wordbar-sense-reset',
            child: new St.Icon({icon_name: 'edit-undo-symbolic', icon_size: 13}),
            can_focus: true,
            visible: false,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._resetSense.connect('clicked', () => this.emit('sense-reset'));
        senseRow.add_child(this._resetSense);

        this.add_child(senseRow);
        this._senseRow = senseRow;
        this._senses = [];

        this._definition = new St.Label({style_class: 'wordbar-definition'});
        this._definition.clutter_text.line_wrap = true;
        this.add_child(this._definition);

        this._example = new St.Label({style_class: 'wordbar-example'});
        this._example.clutter_text.line_wrap = true;
        this.add_child(this._example);

        // Offered instead of an example when enrichment produced nothing, so
        // an offline capture is still a usable card.
        this._addDefinition = new St.Button({
            style_class: 'wordbar-link-button',
            label: _('Add definition'),
            x_align: Clutter.ActorAlign.START,
            can_focus: true,
        });
        this._addDefinition.connect('clicked', () => this.emit('edit-requested'));
        this.add_child(this._addDefinition);

        // Two ratings only. Archiving is housekeeping and lives in the
        // manager; a Learned button here invites premature archiving of words
        // that are still being learned.
        this._actions = new St.BoxLayout({style_class: 'wordbar-actions', x_expand: true});
        this._again = this._actionButton(_('Again'), 'wordbar-action-again', 'rated-again');
        this._know = this._actionButton(_('Know'), 'wordbar-action-know', 'rated-know');
        for (const button of [this._again, this._know])
            this._actions.add_child(button);
        this.add_child(this._actions);

        this._word = null;
    }

    /**
     * Offer the word's other senses.
     *
     * Hidden entirely for a single sense, which is the common case; the
     * switcher must not become permanent furniture.
     */
    setSenses(senses, baselineId = null) {
        this._senses = senses ?? [];
        const many = this._senses.length > 1;
        for (const actor of [this._prevSense, this._nextSense, this._senseCount])
            actor.visible = many;

        if (!many) {
            this._resetSense.visible = false;
            return;
        }

        const index = this._senses.findIndex(s => s.selected);
        this._senseCount.text = `${index < 0 ? 1 : index + 1}/${this._senses.length}`;

        const selected = this._senses[index]?.id ?? null;
        this._resetSense.visible =
            baselineId !== null && selected !== null && selected !== baselineId;
    }

    _stepSense(delta) {
        if (this._senses.length < 2)
            return;
        const current = this._senses.findIndex(s => s.selected);
        const next = (Math.max(0, current) + delta + this._senses.length) % this._senses.length;
        this.emit('sense-selected', this._senses[next].id);
    }

    _actionButton(label, styleClass, signal) {
        const button = new St.Button({
            style_class: `wordbar-action ${styleClass}`,
            label,
            x_expand: true,
            can_focus: true,
        });
        button.connect('clicked', () => this.emit(signal));
        return button;
    }

    get word() {
        return this._word;
    }

    /** Render a word, or the empty state when passed null. */
    setWord(word) {
        this._word = word;

        if (!word) {
            this._term.text = _('No words yet');
            this._senseRow.visible = false;
            this.setSenses([]);
            this._definition.text = _(
                'Select a word anywhere and press Super+Shift+V to add it.\n' +
                'Right-click this indicator for the vocabulary and settings.'
            );
            this._definition.visible = true;
            this._example.visible = false;
            this._addDefinition.visible = false;
            this._speaker.visible = false;
            // Hidden, not greyed out: there is nothing to rate, and a dimmed
            // pair of buttons still reads as something you failed to use.
            this._actions.visible = false;
            return;
        }

        this._actions.visible = true;

        this._term.text = word.term;

        const partOfSpeech = word.part_of_speech || '';
        this._partOfSpeech.text = partOfSpeech;
        this._senseRow.visible = partOfSpeech !== '' || this._senses.length > 1;

        const definition = word.definition || '';
        // Phase 18: a word with no definition is still perfectly usable.
        this._definition.text = definition || _('Definition unavailable');
        this._definition.visible = true;
        this._definition.style_class = definition
            ? 'wordbar-definition'
            : 'wordbar-definition wordbar-definition-missing';

        const example = word.example || '';
        this._example.text = example;
        this._example.visible = example !== '';

        this._addDefinition.visible = definition === '';
        // Offered whenever we can say something: a recording, or a
        // phonetic spelling worth synthesising.
        this._speaker.visible = Boolean(word.has_audio) || Boolean(word.phonetic);
    }
});
