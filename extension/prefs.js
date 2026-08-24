/* prefs.js — extension preferences.
 *
 * Only what a user actually needs. Scheduler internals (retention, stability,
 * difficulty, fuzz) are never exposed: FSRS tunes itself, and exposing knobs
 * would invite people to break their own schedule.
 *
 * Vocabulary editing and import/export live in the Wordbar manager app, which
 * this page links to.
 */

import Adw from 'gi://Adw?version=1';
import Gdk from 'gi://Gdk?version=4.0';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk?version=4.0';

import {ExtensionPreferences, gettext as _}
    from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

const ROTATION_OPTIONS = [5, 10, 15, 30, 60];

export default class WordbarPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();

        const page = new Adw.PreferencesPage({
            title: _('General'),
            icon_name: 'preferences-system-symbolic',
        });
        window.add(page);

        const panelGroup = new Adw.PreferencesGroup({title: _('Panel')});
        page.add(panelGroup);

        const showRow = new Adw.SwitchRow({
            title: _('Show word in top bar'),
            subtitle: _('The panel word is the recall prompt; click it for the meaning.'),
        });
        settings.bind('show-in-top-bar', showRow, 'active', Gio.SettingsBindFlags.DEFAULT);
        panelGroup.add(showRow);

        const rotationRow = new Adw.ComboRow({
            title: _('Change word every'),
            model: Gtk.StringList.new(
                ROTATION_OPTIONS.map(m => _('%d minutes').replace('%d', String(m)))
            ),
        });
        const current = settings.get_int('rotation-minutes');
        const index = ROTATION_OPTIONS.indexOf(current);
        rotationRow.selected = index >= 0 ? index : ROTATION_OPTIONS.indexOf(15);
        rotationRow.connect('notify::selected', row => {
            settings.set_int('rotation-minutes', ROTATION_OPTIONS[row.selected]);
        });
        panelGroup.add(rotationRow);

        const widthRow = new Adw.SpinRow({
            title: _('Maximum characters'),
            subtitle: _('Longer terms are shortened so the top bar does not jump.'),
            adjustment: new Gtk.Adjustment({lower: 6, upper: 60, step_increment: 1}),
        });
        settings.bind('max-panel-chars', widthRow, 'value', Gio.SettingsBindFlags.DEFAULT);
        panelGroup.add(widthRow);

        const captureGroup = new Adw.PreferencesGroup({
            title: _('Capture'),
            description: _('Select a word anywhere, then press the shortcut to add it.'),
        });
        page.add(captureGroup);
        captureGroup.add(this._buildShortcutRow(settings));

        const vocabularyGroup = new Adw.PreferencesGroup({
            title: _('Vocabulary'),
            description: _('Editing, import and export live in the Wordbar app.'),
        });
        page.add(vocabularyGroup);

        const openRow = new Adw.ActionRow({title: _('Open Wordbar')});
        const openButton = new Gtk.Button({
            label: _('Open'),
            valign: Gtk.Align.CENTER,
        });
        openButton.connect('clicked', () => this._openManager());
        openRow.add_suffix(openButton);
        openRow.activatable_widget = openButton;
        vocabularyGroup.add(openRow);
    }

    /** A single-accelerator capture row, edited by pressing the new keys. */
    _buildShortcutRow(settings) {
        const row = new Adw.ActionRow({title: _('Capture shortcut')});

        const label = new Gtk.ShortcutLabel({
            accelerator: settings.get_strv('capture-shortcut')[0] ?? '',
            valign: Gtk.Align.CENTER,
        });
        row.add_suffix(label);

        const button = new Gtk.Button({
            icon_name: 'document-edit-symbolic',
            valign: Gtk.Align.CENTER,
            tooltip_text: _('Set a new shortcut'),
        });
        button.connect('clicked', () => this._captureShortcut(row, settings, label));
        row.add_suffix(button);
        row.activatable_widget = button;
        return row;
    }

    _captureShortcut(row, settings, label) {
        const dialog = new Adw.MessageDialog({
            transient_for: row.get_root(),
            heading: _('Set capture shortcut'),
            body: _('Press the key combination, or Escape to cancel.'),
        });
        dialog.add_response('cancel', _('Cancel'));

        const controller = new Gtk.EventControllerKey();
        controller.connect('key-pressed', (_c, keyval, keycode, state) => {
            if (keyval === Gdk.KEY_Escape) {
                dialog.close();
                return Gdk.EVENT_STOP;
            }

            const mask = state & Gtk.accelerator_get_default_mod_mask();
            // Require a modifier: a bare key would swallow normal typing.
            if (mask === 0 || !Gtk.accelerator_valid(keyval, mask))
                return Gdk.EVENT_STOP;

            const accelerator = Gtk.accelerator_name_with_keycode(null, keyval, keycode, mask);
            settings.set_strv('capture-shortcut', [accelerator]);
            label.accelerator = accelerator;
            dialog.close();
            return Gdk.EVENT_STOP;
        });
        dialog.add_controller(controller);
        dialog.present();
    }

    _openManager() {
        try {
            Gio.DBus.session.call(
                'org.wordbar.Wordbar',
                '/org/wordbar/Wordbar',
                'org.wordbar.Wordbar',
                'OpenVocabulary',
                null, null, Gio.DBusCallFlags.NONE, -1, null,
                (connection, result) => {
                    try {
                        connection.call_finish(result);
                    } catch (error) {
                        console.warn(`Wordbar: cannot open the manager: ${error.message}`);
                    }
                }
            );
        } catch (error) {
            console.warn(`Wordbar: cannot reach the service: ${error.message}`);
        }
    }
}
