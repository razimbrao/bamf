// bamf GNOME Shell extension: a panel icon to pause or resume the bamf
// client, and clipboard sync. It talks to the client over its local control
// socket. GNOME does not let background programs use the Wayland
// clipboard, but the shell itself can.

import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Link} from './link.js';

/** Parses `status connected=1 remote=0 paused=0 server_paused=0`. */
function parseStatus(line) {
    const status = {};
    for (const field of line.split(' ').slice(1)) {
        const [key, value] = field.split('=');
        status[key] = value === '1';
    }
    return status;
}

const TEXT = 'text/plain;charset=utf-8';
const PNG = 'image/png';

/** Watches the clipboard and forwards changes; applies remote content. */
class ClipboardSync {
    constructor(send) {
        this._send = send;
        this._clipboard = St.Clipboard.get_default();
        this._selection = global.display.get_selection();
        this._ownerId = this._selection.connect('owner-changed', (_sel, type) => {
            if (type === Meta.SelectionType.SELECTION_CLIPBOARD)
                this._onChanged();
        });
    }

    destroy() {
        this._selection.disconnect(this._ownerId);
    }

    _onChanged() {
        const type = St.ClipboardType.CLIPBOARD;
        const mimes = this._clipboard.get_mimetypes(type);
        // Text wins over images, as on the Windows side.
        if (mimes.some(m => m.startsWith('text/plain') || m === 'UTF8_STRING' || m === 'STRING')) {
            this._clipboard.get_text(type, (_c, text) => {
                if (text)
                    this._send(`clip ${TEXT} ${GLib.base64_encode(new TextEncoder().encode(text))}`);
            });
        } else if (mimes.includes(PNG)) {
            this._clipboard.get_content(type, PNG, (_c, bytes) => {
                if (bytes && bytes.get_size() > 0)
                    this._send(`clip ${PNG} ${GLib.base64_encode(bytes.get_data())}`);
            });
        }
    }

    /** Sets the clipboard from a `clip <mime> <base64>` line. */
    apply(line) {
        const [, mime, b64 = ''] = line.split(' ');
        const data = GLib.base64_decode(b64);
        const type = St.ClipboardType.CLIPBOARD;
        // The resulting owner-changed is sent back to the client, which
        // recognises its own content and drops it.
        if (mime.startsWith('text/'))
            this._clipboard.set_text(type, new TextDecoder().decode(data));
        else
            this._clipboard.set_content(type, mime, new GLib.Bytes(data));
    }
}

const BamfIndicator = GObject.registerClass(
class BamfIndicator extends PanelMenu.Button {
    _init(onToggle) {
        super._init(0.0, 'bamf');
        this._icon = new St.Icon({
            icon_name: 'action-unavailable-symbolic',
            style_class: 'system-status-icon',
        });
        this.add_child(this._icon);

        this._statusItem = new PopupMenu.PopupMenuItem('', {reactive: false});
        this.menu.addMenuItem(this._statusItem);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._switch = new PopupMenu.PopupSwitchMenuItem('Share keyboard and mouse', true);
        this._switch.connect('toggled', (_item, state) => onToggle(state));
        this.menu.addMenuItem(this._switch);

        this.showDown();
    }

    showDown() {
        this._icon.icon_name = 'action-unavailable-symbolic';
        this._statusItem.label.text = 'bamf-client is not running';
        this._switch.setSensitive(false);
    }

    showStatus(s) {
        let icon, text;
        if (s.paused) {
            [icon, text] = ['media-playback-pause-symbolic', 'Paused here'];
        } else if (!s.connected) {
            [icon, text] = ['network-offline-symbolic', 'Waiting for the server'];
        } else if (s.server_paused) {
            [icon, text] = ['media-playback-pause-symbolic', 'Paused on the server'];
        } else if (s.remote) {
            [icon, text] = ['input-keyboard-symbolic', 'Keyboard and mouse are here'];
        } else {
            [icon, text] = ['input-mouse-symbolic', 'Connected'];
        }
        this._icon.icon_name = icon;
        this._statusItem.label.text = text;
        this._switch.setSensitive(true);
        // setToggleState does not emit `toggled`, so this cannot loop.
        this._switch.setToggleState(!s.paused);
    }
});

export default class BamfExtension extends Extension {
    enable() {
        this._indicator = new BamfIndicator(on => this._link.send(on ? 'resume' : 'pause'));
        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._link = new Link(
            line => this._onLine(line),
            up => {
                if (!up)
                    this._indicator.showDown();
            });
        this._clipboard = new ClipboardSync(line => this._link.send(line));
    }

    disable() {
        this._clipboard.destroy();
        this._clipboard = null;
        this._link.destroy();
        this._link = null;
        this._indicator.destroy();
        this._indicator = null;
    }

    _onLine(line) {
        if (line.startsWith('status '))
            this._indicator.showStatus(parseStatus(line));
        else if (line.startsWith('clip '))
            this._clipboard.apply(line);
    }
}
