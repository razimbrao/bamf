// Line-based connection to the bamf client's control socket
// ($XDG_RUNTIME_DIR/bamf.sock). Reconnects on its own while enabled.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const RECONNECT_SECONDS = 2;

export class Link {
    /**
     * @param {(line: string) => void} onLine called for every line received
     * @param {(up: boolean) => void} onState called when the link goes up or down
     */
    constructor(onLine, onState) {
        this._onLine = onLine;
        this._onState = onState;
        this._path = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'bamf.sock']);
        this._cancellable = new Gio.Cancellable();
        this._conn = null;
        this._queue = [];
        this._writing = false;
        this._retryId = 0;
        this._encoder = new TextEncoder();
        this._connect();
    }

    get up() {
        return this._conn !== null;
    }

    send(line) {
        if (!this._conn)
            return;
        this._queue.push(this._encoder.encode(`${line}\n`));
        this._drain();
    }

    destroy() {
        this._cancellable.cancel();
        if (this._retryId) {
            GLib.source_remove(this._retryId);
            this._retryId = 0;
        }
        this._close(false);
    }

    _connect() {
        const client = new Gio.SocketClient();
        const address = Gio.UnixSocketAddress.new(this._path);
        client.connect_async(address, this._cancellable, (source, result) => {
            try {
                this._conn = source.connect_finish(result);
            } catch (e) {
                if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    this._scheduleReconnect();
                return;
            }
            this._input = new Gio.DataInputStream({
                base_stream: this._conn.get_input_stream(),
                close_base_stream: false,
            });
            this._output = this._conn.get_output_stream();
            this._onState(true);
            this._readLine();
        });
    }

    _scheduleReconnect() {
        if (this._retryId || this._cancellable.is_cancelled())
            return;
        this._retryId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, RECONNECT_SECONDS, () => {
            this._retryId = 0;
            this._connect();
            return GLib.SOURCE_REMOVE;
        });
    }

    _readLine() {
        this._input.read_line_async(GLib.PRIORITY_DEFAULT, this._cancellable, (stream, result) => {
            let line;
            try {
                [line] = stream.read_line_finish_utf8(result);
            } catch (e) {
                if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    this._close(true);
                return;
            }
            if (line === null) {
                this._close(true); // the client went away
                return;
            }
            try {
                this._onLine(line);
            } catch (e) {
                logError(e, 'bamf: handling a line from the client');
            }
            this._readLine();
        });
    }

    _drain() {
        if (this._writing || !this._conn || this._queue.length === 0)
            return;
        this._writing = true;
        const chunk = this._queue.shift();
        this._output.write_all_async(chunk, GLib.PRIORITY_DEFAULT, this._cancellable, (stream, result) => {
            this._writing = false;
            try {
                stream.write_all_finish(result);
            } catch (e) {
                if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    this._close(true);
                return;
            }
            this._drain();
        });
    }

    _close(reconnect) {
        if (this._conn) {
            this._conn.close(null);
            this._conn = null;
            this._queue = [];
            this._writing = false;
            this._onState(false);
        }
        if (reconnect)
            this._scheduleReconnect();
    }
}
