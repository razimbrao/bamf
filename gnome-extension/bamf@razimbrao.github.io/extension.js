// bamf GNOME Shell extension: a panel icon to pause or resume the bamf
// client, and clipboard sync. It talks to the client over its local control
// socket. GNOME does not let background programs use the Wayland
// clipboard, but the shell itself can.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
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
    /**
     * @param {Gio.File} dir the extension directory (for the icons)
     * @param {(on: boolean) => void} onToggle
     */
    _init(dir, onToggle) {
        super._init(0.0, 'bamf');
        this._onToggle = onToggle;
        this._up = false;
        this._paused = false;
        // The same pixel-art imp as the Windows tray, generated from the
        // tray artwork by scripts/gnome_icons.py.
        this._looks = {};
        for (const look of ['awake', 'dozing', 'asleep'])
            this._looks[look] = new Gio.FileIcon({file: dir.get_child('icons').get_child(`bamf-${look}.svg`)});
        this._icon = new St.Icon({
            gicon: this._looks.dozing,
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

    /**
     * Left click pauses or resumes right away; right click (or any click
     * while the client is not running) opens the menu.
     */
    vfunc_event(event) {
        if (this._up &&
            event.type() === Clutter.EventType.BUTTON_PRESS &&
            event.get_button() === Clutter.BUTTON_PRIMARY) {
            this._onToggle(this._paused);
            return Clutter.EVENT_STOP;
        }
        return super.vfunc_event(event);
    }

    showDown() {
        this._up = false;
        this._icon.gicon = this._looks.dozing;
        this._statusItem.label.text = 'bamf-client is not running';
        this._switch.setSensitive(false);
    }

    showStatus(s) {
        let look, text;
        // The pause state is shared with the server: flipping it here or
        // on Windows changes it on both machines. Same faces as the tray:
        // awake = ready, dozing = waiting, asleep = paused.
        if (!s.connected) {
            [look, text] = ['dozing', 'Waiting for the server'];
        } else if (s.paused) {
            [look, text] = ['asleep', 'Paused on both machines'];
        } else if (s.remote) {
            [look, text] = ['awake', 'Keyboard and mouse are here'];
        } else {
            [look, text] = ['awake', 'Connected'];
        }
        this._icon.gicon = this._looks[look];
        this._up = true;
        this._paused = s.paused;
        this._statusItem.label.text = text;
        this._switch.setSensitive(true);
        // setToggleState does not emit `toggled`, so this cannot loop.
        this._switch.setToggleState(!s.paused);
    }
});

export default class BamfExtension extends Extension {
    enable() {
        this._indicator = new BamfIndicator(this.dir, on => this._link.send(on ? 'resume' : 'pause'));
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
