/* toast.js — brief, reversible confirmations.
 *
 * Built from St widgets rather than MessageTray notifications: capture
 * feedback must not survive in the notification list ("save now, learn later"
 * means the user is going straight back to reading), and the inline Undo
 * button needs a stable API across Shell versions.
 */

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {gettext as _} from 'resource:///org/gnome/shell/extensions/extension.js';

const DEFAULT_TIMEOUT = 5;

export const Toast = GObject.registerClass({
    GTypeName: 'WordbarToast',
}, class Toast extends St.BoxLayout {
    /**
     * @param {string} message  text to show
     * @param {?{label: string, callback: function}} action  optional button
     * @param {number} timeout  seconds before it fades out
     */
    _init(message, action = null, timeout = DEFAULT_TIMEOUT) {
        super._init({
            style_class: 'wordbar-toast',
            vertical: false,
            reactive: true,
            track_hover: true,
        });

        this.add_child(new St.Label({
            style_class: 'wordbar-toast-label',
            text: message,
            y_align: Clutter.ActorAlign.CENTER,
        }));

        if (action) {
            const button = new St.Button({
                style_class: 'wordbar-toast-action',
                label: action.label,
                y_align: Clutter.ActorAlign.CENTER,
                can_focus: true,
            });
            button.connect('clicked', () => {
                this.dismiss();
                action.callback();
            });
            this.add_child(button);
        }

        Main.layoutManager.addTopChrome(this);
        this._reposition();
        this._monitorsId = Main.layoutManager.connect('monitors-changed', () => this._reposition());

        this.opacity = 0;
        this.ease({
            opacity: 255,
            duration: 150,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });

        // Hovering keeps it around: the user is probably reaching for Undo.
        this._timeout = timeout;
        this._startTimer();
        this.connect('notify::hover', () => {
            if (this.hover)
                this._stopTimer();
            else
                this._startTimer();
        });
        this.connect('destroy', () => this._onDestroy());
    }

    /**
     * Sit just under the top bar, near the indicator.
     *
     * That ties the confirmation to where the word will actually appear,
     * rather than announcing it at the far end of the screen.
     */
    _reposition() {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;

        const panel = Main.panel;
        const top = monitor.y + (panel ? panel.height : 0) + 8;

        // Prefer to line up with the indicator, but never leave the monitor.
        let left = monitor.x + Math.floor((monitor.width - this.width) / 2);
        const indicator = Main.panel.statusArea['wordbar@kemalbektas.dev'];
        if (indicator) {
            const [x] = indicator.get_transformed_position();
            if (!Number.isNaN(x))
                left = Math.round(x - this.width / 2 + indicator.width / 2);
        }
        const maxLeft = monitor.x + monitor.width - this.width - 8;
        left = Math.max(monitor.x + 8, Math.min(left, maxLeft));

        this.set_position(left, top);
    }

    _startTimer() {
        this._stopTimer();
        this._timerId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, this._timeout, () => {
            this._timerId = 0;
            this.dismiss();
            return GLib.SOURCE_REMOVE;
        });
    }

    _stopTimer() {
        if (this._timerId) {
            GLib.Source.remove(this._timerId);
            this._timerId = 0;
        }
    }

    dismiss() {
        if (this._dismissing)
            return;
        this._dismissing = true;
        this._stopTimer();
        this.ease({
            opacity: 0,
            duration: 150,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => this.destroy(),
        });
    }

    _onDestroy() {
        this._stopTimer();
        if (this._monitorsId) {
            Main.layoutManager.disconnect(this._monitorsId);
            this._monitorsId = 0;
        }
    }
});

/** Track the visible toast so confirmations replace rather than stack. */
let current = null;

export function showToast(message, action = null, timeout = DEFAULT_TIMEOUT) {
    if (current) {
        current.dismiss();
        current = null;
    }
    const toast = new Toast(message, action, timeout);
    current = toast;
    toast.connect('destroy', () => {
        if (current === toast)
            current = null;
    });
    return toast;
}

export function showUndoToast(message, undoCallback) {
    return showToast(message, {label: _('Undo'), callback: undoCallback});
}

export function dismissToast() {
    if (current) {
        current.dismiss();
        current = null;
    }
}
