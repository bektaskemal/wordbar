/* capture.js — Super+Shift+V quick capture.
 *
 * Flow:
 *   PRIMARY selection holds a usable term  -> save immediately
 *   otherwise, the clipboard holds one     -> prefilled Add dialog
 *   otherwise                              -> empty Add dialog
 *
 * Clipboard content is never saved without confirmation: it is often stale,
 * and a silently added wrong word is worse than one extra keystroke.
 */

import St from 'gi://St';

import {AddWordDialog} from './adddialog.js';
import {looksLikeTerm} from './terms.js';

/** Read one clipboard target asynchronously. */
function readClipboard(type) {
    return new Promise(resolve => {
        St.Clipboard.get_default().get_text(type, (clipboard, text) => resolve(text ?? ''));
    });
}

export {looksLikeTerm};

/**
 * Run the capture flow.
 *
 * @param {object} handlers
 * @param {function(string): void} handlers.onCapture  called with the term to add
 * @param {function(): void} [handlers.onDialogOpened]
 */
export async function capture({onCapture, onDialogOpened}) {
    const primary = await readClipboard(St.ClipboardType.PRIMARY);
    if (looksLikeTerm(primary)) {
        onCapture(primary.trim());
        return;
    }

    const clipboard = await readClipboard(St.ClipboardType.CLIPBOARD);
    const prefill = looksLikeTerm(clipboard) ? clipboard.trim() : '';

    showAddDialog(prefill, onCapture);
    onDialogOpened?.();
}

/** Open the Add dialog, optionally prefilled. */
export function showAddDialog(prefill, onCapture) {
    const dialog = new AddWordDialog(prefill);
    dialog.connect('word-confirmed', (_dialog, text) => onCapture(text));
    dialog.open(global.get_current_time());
    return dialog;
}
