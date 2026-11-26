# Architecture

```
┌──────────── Windows (server) ────────────┐            ┌──────── Fedora (client) ────────┐
│                                          │            │                                 │
│  WH_MOUSE_LL / WH_KEYBOARD_LL hooks      │            │  UDP socket                     │
│        │                                 │  UDP       │        │                        │
│        ▼                                 │ encrypted  │        ▼                        │
│  Switch state machine ──► protocol ──────┼───────────►│  protocol ──► injector          │
│  (local / remote)        encode+seal     │            │  open+decode    │               │
│        ▲                                 │◄───────────┼── Ping (500 ms) ▼               │
│        │ heartbeat thread (200 ms)       │            │           /dev/uinput           │
│        └─ liveness, key state sync       │            │   (keyboard, mouse, pointer)    │
└──────────────────────────────────────────┘            └─────────────────────────────────┘
```

## Workspace layout

| Crate | Purpose |
|---|---|
| `bamf-proto` | Wire messages, binary encoding, encryption, key-state bitmap. Platform-independent and unit-tested. |
| `bamf-server` | Windows binary. Input capture, edge detection, cursor handling. |
| `bamf-client` | Linux binary. uinput virtual devices and event injection. |

Each binary compiles on any OS but only does real work on its target OS; on
other platforms `main` prints an error. The workspace has no third-party
dependencies; see [development.md](development.md#zero-dependencies).

Inside the server, `controller.rs` (state machine), `geometry.rs` (edges and
virtual cursor) and `keymap.rs` are pure logic; `windows/` holds the FFI glue.
Inside the client, `state.rs` turns messages into input actions and `linux/`
writes them to uinput.

## Server (Windows)

### Input capture

Low-level hooks (`SetWindowsHookExW` with `WH_MOUSE_LL` and `WH_KEYBOARD_LL`)
run on a dedicated thread with a message loop. A hook returning a non-zero
value **suppresses** the event, so Windows never sees it. Callbacks must stay
fast: Windows silently removes hooks that exceed `LowLevelHooksTimeout`.

Events flagged as injected (`LLMHF_INJECTED`, `LLKHF_INJECTED`) are ignored.

### State machine

```
            cursor touches the configured edge
            AND client is alive
   LOCAL ───────────────────────────────────────► REMOTE
     ▲                                               │
     │  virtual cursor leaves the remote screen      │
     │  through the opposite edge, OR Scroll Lock,   │
     └──── OR client stops pinging (> 2 s) ──────────┘
```

**Entering remote mode**

1. Compute the virtual cursor position on the remote screen, keeping the same
   relative position along the shared edge.
2. Hide the Windows cursor (replace system cursors with a blank one via
   `SetSystemCursor`; restored with `SystemParametersInfoW(SPI_SETCURSORS)`).
3. Park the real cursor at the centre of the primary monitor.
4. Send `Enter`.

**While remote**

- Mouse move: the hook receives the position the cursor *would* move to
  (after Windows pointer acceleration). `delta = pt - parked_position`.
  The event is suppressed, so the real cursor never moves and the next delta
  is again relative to the parked position.
- The virtual cursor is updated, clamped to the remote screen, and sent either
  as an absolute position (`MouseAbs`, default) or as a delta (`MouseRel`).
- Buttons, wheel and keys are forwarded and suppressed.
- *Release pass-through*: a key/button **release** for something that was
  not pressed in remote mode (e.g. Ctrl held before crossing) is passed to
  Windows instead of the client, so Windows never keeps a stuck key.

**Leaving remote mode**

Restore cursors, place the real cursor just inside the edge it came from at
the matching position, send `Leave`.

### Liveness

The client pings every 500 ms. The server only enters remote mode if a ping
arrived in the last 2 s, and a heartbeat thread forces `LOCAL` if pings stop.
While remote, the same thread sends a `State` snapshot every 200 ms.

## Client (Linux)

### Injection via uinput

The client creates virtual devices through `/dev/uinput`. This works on any
Wayland compositor and on X11, because events enter at the kernel level and
are processed by libinput like real hardware.

| Device | Capabilities | Used for |
|---|---|---|
| `bamf keyboard` | all keys | keyboard |
| `bamf mouse` | `REL_X/Y`, wheels, buttons | `pointer_mode = "relative"` |
| `bamf pointer` | `ABS_X/Y` (0–65535), wheels, buttons | `pointer_mode = "absolute"` (like a VM "tablet") |

Absolute mode lets the server own the cursor position, so it knows exactly
when the cursor reaches the return edge, independent of GNOME pointer
acceleration and without asking the compositor (Wayland does not expose the
global cursor position).

Access to `/dev/uinput` is granted to the logged-in user through a udev rule
with `TAG+="uaccess"` (see the README), avoiding the `input` group, which
would also allow reading every input device.

### Fail-safes

- `Leave` or `State` with an empty set → release keys/buttons accordingly.
- No packet from the server for 1 s while in remote mode → release everything.

## Pause and resume

Pause is one shared switch, stored on the server and changed from either
side: the tray icon on Windows
(`windows/tray.rs`, a hidden window plus `Shell_NotifyIconW`), and on Linux
the client's control socket (`$XDG_RUNTIME_DIR/bamf.sock`, a line protocol
described in `bamf-client/src/control.rs`), driven by the GNOME Shell
extension or `bamf-client pause|resume|toggle`. A switch flipped on the
client rides on its pings until the server's reply confirms it; every reply
carries the state, so both icons always agree.

## Clipboard

```
 Windows                          TCP (encrypted)        Linux
 WM_CLIPBOARDUPDATE ─► read ─► convert ─► send ════► client ─► socket ─► extension ─► St.Clipboard
 SetClipboardData ◄── convert ◄── receive ◄════════ client ◄─ socket ◄─ extension ◄─ owner-changed
```

- Windows (`windows/clipboard.rs`, `windows/sync.rs`): the main thread only
  copies the clipboard out; conversion and network I/O run on their own
  threads so input is never delayed. Text wins over images. Images are read
  from the registered `PNG` format when an application offers it, otherwise
  from `CF_DIB`, and written as both.
- Linux: GNOME does not give background programs access to the Wayland
  clipboard, so the shell extension watches it (`Meta.Selection`
  `owner-changed`) and sets it (`St.Clipboard`), exchanging base64 lines
  with the client.
- `bamf-server/src/image/` holds DEFLATE (fixed-Huffman LZ77), inflate,
  PNG and DIB code, all tested against data produced by Python's zlib.

## Key codes

The protocol carries **Linux evdev key codes** (`input-event-codes.h`) as the
canonical representation. For PC set-1 scancodes `0x01–0x58`, the evdev code is
numerically identical; extended (`E0`-prefixed) and a few special keys use a
small table (`bamf-server/src/keymap.rs`). Characters and layouts are never
transmitted: both machines use their own layout (ABNT2), so a physical key
always means the same thing.

## Security

All datagrams are encrypted and authenticated with ChaCha20-Poly1305
(implemented in `bamf-proto/src/aead.rs`, RFC 8439) using a
32-byte pre-shared key (generated with `bamf-server keygen`). See
[protocol.md](protocol.md) for nonce and replay handling. Without the key,
packets are dropped before decoding. Keystrokes, including passwords, never
travel in clear text.
