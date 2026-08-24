/* adddialog.js — the small prefilled Add dialog.
 *
 * Shown when capture falls back to the clipboard, or when the user adds a word
 * by hand. Never shown for a PRIMARY-selection capture: that path is
 * unambiguous and should not interrupt reading.
 */

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Dialog from 'resource:///org/gnome/shell/ui/dialog.js';
import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';
import {gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

export const AddWordDialog = GObject.registerClass({
    GTypeName: 'WordbarAddWordDialog',
    Signals: {
        'word-confirmed': {param_types: [GObject.TYPE_STRING]},
    },
}, class AddWordDialog extends ModalDialog.ModalDialog {
    _init(prefill = '') {
        super._init({styleClass: 'wordbar-add-dialog'});

        const content = new Dialog.MessageDialogContent({title: _('Add word')});
        this.contentLayout.add_child(content);

        this._entry = new St.Entry({
            style_class: 'wordbar-add-entry',
            can_focus: true,
            x_expand: true,
            text: prefill,
        });
        this._entry.clutter_text.set_selection(0, prefill.length);
        this._entry.clutter_text.connect('activate', () => this._confirm());
        content.add_child(this._entry);

        this.setButtons([
            {
                label: _('Cancel'),
                action: () => this.close(),
                key: Clutter.KEY_Escape,
            },
            {
                label: _('Add'),
                action: () => this._confirm(),
                default: true,
            },
        ]);
    }

    _confirm() {
        const text = this._entry.get_text().trim();
        this.close();
        if (text)
            this.emit('word-confirmed', text);
    }

    open(timestamp, onPrimary) {
        const opened = super.open(timestamp, onPrimary);
        if (opened)
            this._entry.grab_key_focus();
        return opened;
    }
});
