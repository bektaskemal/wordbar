/* prefs.js — a signpost, not a second settings window.
 *
 * Every Wordbar setting lives in the manager app: panel, rotation, capture
 * shortcut, import and export. GNOME still offers this page from the
 * Extensions app, so rather than maintain two views of the same GSettings it
 * shows what is configurable and opens the one place to do it.
 */

import Adw from 'gi://Adw?version=1';
import Gio from 'gi://Gio';
import Gtk from 'gi://Gtk?version=4.0';

import {ExtensionPreferences, gettext as _}
    from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

export default class WordbarPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const page = new Adw.PreferencesPage({
            title: _('Wordbar'),
            icon_name: 'preferences-system-symbolic',
        });
        window.add(page);

        const group = new Adw.PreferencesGroup({
            title: _('Settings live in the Wordbar app'),
            description: _(
                'Panel, rotation interval, capture shortcut, import and export are all ' +
                'there, so there is only one place to look.'
            ),
        });
        page.add(group);

        const row = new Adw.ActionRow({
            title: _('Open Wordbar'),
            subtitle: _('Or right-click the word in the top bar'),
        });
        const button = new Gtk.Button({
            label: _('Open'),
            valign: Gtk.Align.CENTER,
            css_classes: ['suggested-action'],
        });
        button.connect('clicked', () => this._openManager(window));
        row.add_suffix(button);
        row.activatable_widget = button;
        group.add(row);

        this._addStatusRow(group);
    }

    /** A quick reminder of what is currently configured. */
    _addStatusRow(group) {
        const settings = this.getSettings();

        const shortcut = new Adw.ActionRow({title: _('Capture shortcut')});
        shortcut.add_suffix(new Gtk.ShortcutLabel({
            accelerator: settings.get_strv('capture-shortcut')[0] ?? '',
            valign: Gtk.Align.CENTER,
        }));
        group.add(shortcut);

        const rotation = new Adw.ActionRow({title: _('Change word every')});
        const label = new Gtk.Label({
            label: _('%d minutes').replace('%d', String(settings.get_int('rotation-minutes'))),
            css_classes: ['dim-label'],
            valign: Gtk.Align.CENTER,
        });
        settings.connect('changed::rotation-minutes', () => {
            label.label = _('%d minutes')
                .replace('%d', String(settings.get_int('rotation-minutes')));
        });
        rotation.add_suffix(label);
        group.add(rotation);
    }

    _openManager(window) {
        Gio.DBus.session.call(
            'org.wordbar.Wordbar',
            '/org/wordbar/Wordbar',
            'org.wordbar.Wordbar',
            'OpenPreferences',
            null, null, Gio.DBusCallFlags.NONE, -1, null,
            (connection, result) => {
                try {
                    connection.call_finish(result);
                    window.close();
                } catch (error) {
                    console.warn(`Wordbar: cannot open the manager: ${error.message}`);
                }
            }
        );
    }
}
