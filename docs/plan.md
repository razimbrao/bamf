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
| Windows server | Done, builds; core logic unit-tested; not yet tried with a real client |
| Linux client | Done, type-checked for Linux; state logic unit-tested; not yet run on Fedora |
| Phase 0 hardware validation (R1–R5) | **Pending**: needs both machines |

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

- Clipboard sharing (text). Hard on GNOME/Wayland: no `wlr-data-control`.
- Run as a background service (Windows startup task, systemd user unit).
- Tray icon / status notifications.
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
