# bamf — Project Plan

> *bamf* — the sound Nightcrawler makes when he teleports. Here, it is your
> mouse cursor jumping from one computer to another.

## Goal

Share a single keyboard and mouse between two computers over the local
network. Moving the cursor past the edge of one screen makes it appear on the
other machine, as if it were a second monitor. No hardware switch press needed.

This is a **study project**: written from scratch in Rust. Existing open-source
tools (Lan Mouse, Deskflow, Input Leap) are used only as inspiration for ideas,
never as code sources.

## Target setup

| Machine | OS | Network | Role |
|---|---|---|---|
| Desktop | Windows 11 | Ethernet | **Server** — keyboard and mouse are physically attached here; captures input |
| Laptop | Fedora (GNOME, Wayland) | Wi-Fi | **Client** — receives input and injects it locally |

A UGREEN 4-in-1 USB 2.0 sharing switch is also available. It is **not**
controlled by bamf; it remains the fallback for cases bamf does not cover:

- the desktop is off;
- the laptop is plugged via HDMI into the desktop monitor (in that mode only
  the laptop is used, so the peripherals are moved with the hardware switch);
- BIOS, bootloader or disk-encryption prompts on the laptop.

## Scope

### In scope (MVP)

- One-way control: Windows server → Linux client.
- Screen-edge switching, configurable side (`left`, `right`, `top`, `bottom`).
- Mouse movement, buttons (left, right, middle, back, forward), vertical and
  horizontal wheel.
- Full keyboard, including ABNT2-specific keys. Layout is applied by each OS
  (we send physical key codes, not characters).
- Encrypted and authenticated traffic using a pre-shared key.
- Fail-safes: no stuck keys, input always returns to Windows if the laptop
  disappears, a panic hotkey (Scroll Lock) to return immediately.

### Out of scope (for now)

- Linux → Windows direction (capturing input on Wayland).
- Clipboard sharing, file drag-and-drop.
- More than two machines.
- Auto-discovery, GUI, tray icon.

## Status

| Item | State |
|---|---|
| Protocol, encryption, config | Done, unit-tested (RFC 8439 vectors) |
| Windows server | Done; tested with the real client |
| Linux client | Done; tested on Fedora 43 (GNOME, Wayland) |
| Phase 0 hardware validation | Done: R1–R5 passed |

### Phase 0 results (2026-11-26)

Setup: Windows 11 server on Ethernet, Fedora 43 laptop client on Wi-Fi,
same /24, laptop screen 1920x1080 at 125 % scale (1536x864 logical).

- **R1 passed.** The absolute pointer covers the whole GNOME desktop; no need
  for the relative fallback. One gap found and fixed: GNOME pointer barriers
  (dock reveal at the bottom edge, hot corner) need the cursor to keep
  *pushing* against the edge, which an absolute position cannot express. The
  server now also sends the movement lost to clamping as a relative delta.
- **R2 passed.** No noticeable lag over Wi-Fi.
- **R3 passed.** Crossing, keyboard (ABNT2) and the return edge work; Windows
  does not react while in remote mode.
- **R4 passed.** Crossing works with an elevated window focused.
- **R5 passed.** Holding Shift and turning Wi-Fi off on the laptop: the client
  released the key on its own; typing afterwards produced lowercase letters.

## Roadmap

### Phase 0 — Spike / validation

Validate the risky assumptions with the smallest possible code.

| # | Assumption | How to validate | Acceptance |
|---|---|---|---|
| R1 | A uinput **absolute** pointer is mapped to the whole GNOME desktop | Move the virtual pointer to the four corners | Cursor reaches all edges exactly |
| R2 | Wi-Fi latency is acceptable | Use the mouse for a few minutes; check `ping` p99 | No visible lag or jumps (< 10 ms typical) |
| R3 | Windows low-level hooks can capture and suppress input | Run the server and cross the edge | Windows does not react while in remote mode |
| R4 | Elevated (admin) windows block the hooks | Focus an admin window and cross the edge | Documented: run the server elevated if needed |
| R5 | No stuck keys on packet loss | Disconnect Wi-Fi while holding Shift | Client releases everything within 1 s |

Fallback for R1: `pointer_mode = "relative"` (classic relative mouse device).

### Phase 1 — MVP

Everything listed under *In scope (MVP)*. Done when the author uses it for a
full workday without falling back to the hardware switch.

### Phase 2 — Quality of life

Done (2026-11-26):

- **Screen alignment**: `remote_offset` maps the shared edge pixel to pixel,
  like two physical monitors; outside the overlap the edge is a wall.
- **Pause/resume from both sides** (protocol v2): Windows tray icon,
  GNOME Shell extension with a panel icon, `bamf-client pause|resume|toggle`.
- **Clipboard sync, text and images, both ways**, over an encrypted TCP
  stream. GNOME's Mutter exposes no data-control protocol (checked on
  GNOME 49: only the focus-bound `wl_data_device` and primary selection),
  so the Linux side goes through the shell extension, which can use the
  clipboard without focus. Images travel as PNG; DEFLATE, PNG and DIB
  conversion are implemented from scratch.

Next:

- Custom icon artwork (tray and panel).
- Run as a background service (Windows startup task, systemd user unit),
  without a console window.
- LAN discovery and pairing UX instead of copying a key by hand.

### Phase 3 — Stretch

- Bidirectional control (Linux server via the XDG InputCapture portal + libei).
- More than two machines and a screen layout configuration.

## Known limitations

- `Ctrl+Alt+Del` and `Win+L` are handled by Windows' secure desktop and can
  never be intercepted.
- Low-level hooks do not see input while an elevated window is focused,
  unless the server itself runs elevated.
- If the server crashes while in remote mode, the Windows cursor may stay
  hidden. Restarting the server restores it.
- Clipboard on Linux needs the GNOME Shell extension; other desktops would
  need their own integration.
- Clipboard content above about 48 MiB is not synced. Pausing stops input
  sharing only; the clipboard keeps syncing while both sides run.
