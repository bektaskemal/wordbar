/* backend.js — every D-Bus call the extension makes.
 *
 * Hard rule: GNOME Shell runs the compositor on this thread, so nothing here
 * is ever synchronous. There are no *Sync proxy calls, no file access and no
 * networking; the Wordbar service does all of that out of process.
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';

const BUS_NAME = 'org.wordbar.Wordbar';
const OBJECT_PATH = '/org/wordbar/Wordbar';

/* Only the members the Shell actually uses. */
const IFACE = `
<node>
  <interface name="org.wordbar.Wordbar">
    <method name="AddWord">
      <arg name="text" type="s" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="GetCurrentWord">
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="AdvanceDisplay">
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="GetWordDetails">
      <arg name="id" type="x" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="RateAgain">
      <arg name="id" type="x" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="RateKnow">
      <arg name="id" type="x" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="MarkLearned">
      <arg name="id" type="x" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="RestoreWord">
      <arg name="id" type="x" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="DeleteWord">
      <arg name="id" type="x" direction="in"/>
    </method>
    <method name="OpenVocabulary"/>
    <method name="OpenWordEditor">
      <arg name="id" type="x" direction="in"/>
    </method>
    <method name="ListSenses">
      <arg name="id" type="x" direction="in"/>
      <arg type="aa{sv}" direction="out"/>
    </method>
    <method name="SelectSense">
      <arg name="sense_id" type="x" direction="in"/>
      <arg type="a{sv}" direction="out"/>
    </method>
    <method name="Speak">
      <arg name="id" type="x" direction="in"/>
    </method>
    <method name="OpenPreferences"/>
    <method name="OpenIncomplete"/>
    <method name="Quit"/>
    <method name="CountIncomplete">
      <arg type="u" direction="out"/>
    </method>
    <method name="OpenPreferences"/>
    <method name="Ping">
      <arg type="s" direction="out"/>
    </method>
    <signal name="CurrentWordChanged">
      <arg name="word" type="a{sv}"/>
    </signal>
    <signal name="WordAdded">
      <arg name="id" type="x"/>
    </signal>
    <signal name="WordUpdated">
      <arg name="id" type="x"/>
    </signal>
    <signal name="VocabularyChanged"/>
  </interface>
</node>`;

const WordbarProxy = Gio.DBusProxy.makeProxyWrapper(IFACE);

/** D-Bus error name for "the term is already in the vocabulary". */
export const ERROR_DUPLICATE = 'org.wordbar.Error.Duplicate';

/** Extract the bare D-Bus error name from a GLib error, or null. */
export function errorName(error) {
    if (!error)
        return null;
    try {
        if (Gio.DBusError.is_remote_error(error))
            return Gio.DBusError.get_remote_error(error);
    } catch (_error) {
        // Not a remote error (transport failure, cancellation, ...).
    }
    return null;
}

/**
 * Unpack an `a{sv}` word into a plain object.
 *
 * The service sends empty strings rather than omitting fields, so callers can
 * treat "" as "not known yet" without checking for key presence.
 */
export function unpackWord(variant) {
    if (!variant)
        return null;
    const raw = variant.deepUnpack ? variant.deepUnpack() : variant;
    const keys = Object.keys(raw);
    if (keys.length === 0)
        return null;

    const word = {};
    for (const key of keys) {
        const value = raw[key];
        word[key] = value instanceof GLib.Variant ? value.deepUnpack() : value;
    }
    return word.term ? word : null;
}

/** Unpack one `a{sv}` sense into a plain object. */
function unpackSense(variant) {
    const raw = variant?.deepUnpack ? variant.deepUnpack() : variant;
    if (!raw)
        return null;
    const sense = {};
    for (const key of Object.keys(raw)) {
        const value = raw[key];
        sense[key] = value instanceof GLib.Variant ? value.deepUnpack() : value;
    }
    return sense.definition ? sense : null;
}

/**
 * Async client for the Wordbar service.
 *
 * The proxy is created lazily and asynchronously. If the service cannot be
 * reached, calls reject and the caller degrades gracefully — the Shell must
 * never block or freeze waiting for a backend.
 */
export const Backend = GObject.registerClass({
    GTypeName: 'WordbarBackend',
    Signals: {
        'current-word-changed': {param_types: [GObject.TYPE_JSOBJECT]},
        'word-updated': {param_types: [GObject.TYPE_INT64]},
        'vocabulary-changed': {},
        'availability-changed': {param_types: [GObject.TYPE_BOOLEAN]},
    },
}, class Backend extends GObject.Object {
    _init() {
        super._init();
        this._proxy = null;
        this._pending = null;
        this._signalIds = [];
        this._available = false;
        this._retrySource = 0;
        this._retryDelay = 2;
        this._destroyed = false;
    }

    get available() {
        return this._available;
    }

    /**
     * Resolve to a ready proxy, creating it if necessary.
     *
     * Concurrent callers share one in-flight construction, so a burst of calls
     * during startup does not spawn several proxies.
     */
    _getProxy() {
        if (this._destroyed)
            return Promise.reject(new Error('Wordbar backend destroyed'));
        if (this._proxy)
            return Promise.resolve(this._proxy);
        if (this._pending)
            return this._pending;

        this._pending = new Promise((resolve, reject) => {
            // NONE (rather than DO_NOT_AUTO_START) is what lets the session bus
            // activate the service on the first call.
            new WordbarProxy(
                Gio.DBus.session, BUS_NAME, OBJECT_PATH,
                (proxy, error) => {
                    this._pending = null;
                    if (error) {
                        this._setAvailable(false);
                        this._scheduleRetry();
                        reject(error);
                        return;
                    }
                    if (this._destroyed) {
                        reject(new Error('Wordbar backend destroyed'));
                        return;
                    }
                    this._proxy = proxy;
                    this._connectSignals(proxy);
                    resolve(proxy);
                },
                null,
                Gio.DBusProxyFlags.NONE
            );
        });
        return this._pending;
    }

    _connectSignals(proxy) {
        this._signalIds.push(proxy.connectSignal('CurrentWordChanged', (p, sender, [word]) => {
            this._setAvailable(true);
            this.emit('current-word-changed', unpackWord(word) ?? {});
        }));
        this._signalIds.push(proxy.connectSignal('WordUpdated', (p, sender, [id]) => {
            this.emit('word-updated', id);
        }));
        this._signalIds.push(proxy.connectSignal('VocabularyChanged', () => {
            this.emit('vocabulary-changed');
        }));

        // The service may exit (crash, upgrade). Drop the proxy so the next
        // call re-activates it instead of failing forever.
        this._nameOwnerId = proxy.connect('notify::g-name-owner', () => {
            if (!proxy.g_name_owner)
                this._setAvailable(false);
        });
    }

    _setAvailable(available) {
        if (this._available === available)
            return;
        this._available = available;
        this.emit('availability-changed', available);
    }

    /** Retry with a backoff so a missing backend costs nothing while idle. */
    _scheduleRetry() {
        if (this._retrySource || this._destroyed)
            return;
        this._retrySource = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, this._retryDelay, () => {
            this._retrySource = 0;
            this._retryDelay = Math.min(this._retryDelay * 2, 60);
            this.ping().catch(() => {});
            return GLib.SOURCE_REMOVE;
        });
    }

    /** Call a method by name; resolves with the reply arguments array. */
    async _call(method, ...args) {
        const proxy = await this._getProxy();
        return new Promise((resolve, reject) => {
            proxy[`${method}Remote`](...args, (result, error) => {
                if (error) {
                    // A transport-level failure means the service went away.
                    if (!errorName(error))
                        this._setAvailable(false);
                    reject(error);
                    return;
                }
                this._retryDelay = 2;
                this._setAvailable(true);
                resolve(result);
            });
        });
    }

    async ping() {
        const [version] = await this._call('Ping');
        return version;
    }

    async addWord(text) {
        const [word] = await this._call('AddWord', text);
        return unpackWord(word);
    }

    async getCurrentWord() {
        const [word] = await this._call('GetCurrentWord');
        return unpackWord(word);
    }

    async advanceDisplay() {
        const [word] = await this._call('AdvanceDisplay');
        return unpackWord(word);
    }

    async getWordDetails(id) {
        const [word] = await this._call('GetWordDetails', id);
        return unpackWord(word);
    }

    async rateAgain(id) {
        const [word] = await this._call('RateAgain', id);
        return unpackWord(word);
    }

    async rateKnow(id) {
        const [word] = await this._call('RateKnow', id);
        return unpackWord(word);
    }

    async markLearned(id) {
        const [word] = await this._call('MarkLearned', id);
        return unpackWord(word);
    }

    async restoreWord(id) {
        const [word] = await this._call('RestoreWord', id);
        return unpackWord(word);
    }

    async deleteWord(id) {
        await this._call('DeleteWord', id);
    }

    async openVocabulary() {
        await this._call('OpenVocabulary');
    }

    async openWordEditor(id) {
        await this._call('OpenWordEditor', id);
    }

    async listSenses(id) {
        const [senses] = await this._call('ListSenses', id);
        return senses.map(unpackSense).filter(s => s !== null);
    }

    async selectSense(senseId) {
        const [word] = await this._call('SelectSense', senseId);
        return unpackWord(word);
    }

    async speak(id) {
        await this._call('Speak', id);
    }

    async openPreferences() {
        await this._call('OpenPreferences');
    }

    /* Stop the service. It is D-Bus activated, so the next call restarts it. */
    async quit() {
        await this._call('Quit');
    }

    async openIncomplete() {
        await this._call('OpenIncomplete');
    }

    async countIncomplete() {
        const [count] = await this._call('CountIncomplete');
        return count;
    }

    destroy() {
        this._destroyed = true;
        if (this._retrySource) {
            GLib.Source.remove(this._retrySource);
            this._retrySource = 0;
        }
        if (this._proxy) {
            for (const id of this._signalIds)
                this._proxy.disconnectSignal(id);
            if (this._nameOwnerId)
                this._proxy.disconnect(this._nameOwnerId);
            this._proxy = null;
        }
        this._signalIds = [];
        this._pending = null;
        this._available = false;
    }
});
