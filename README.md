# bamf

> *BAMF!*

**bamf** teleports your keyboard and mouse between two computers over the
local network. Push the cursor past the edge of your Windows screen and it
pops out on your Linux laptop, as if the laptop were a second monitor.

It is a study project, written from scratch in Rust with **zero third-party
dependencies**: Win32 hooks, Linux uinput and ChaCha20-Poly1305 are all
implemented directly on top of the standard library.

```
 Windows desktop (server)                      Fedora laptop (client)
 ┌──────────────────────┐   encrypted UDP     ┌──────────────────────┐
 │ keyboard + mouse     │ ──────────────────► │ virtual input devices│
 │ low-level hooks      │ ◄── ping ────────── │ (/dev/uinput)        │
 └──────────────────────┘                     └──────────────────────┘
```

## Status

Working on Windows 11 → Fedora (GNOME, Wayland):

- keyboard (including ABNT2 keys), mouse movement, five buttons and both
  wheels, edge switching, encryption, stuck-key protection, a panic key;
- **pause/resume from either side**: a tray icon on Windows, a panel icon
  (GNOME Shell extension) or `bamf-client pause|resume|toggle` on Linux;
- **clipboard sync both ways**, text and images;
- optional pixel-to-pixel alignment of the two screens (`remote_offset`).

See [docs/plan.md](docs/plan.md) for the roadmap.

## Quick start

### 1. Build

```powershell
# Windows
cargo build --release -p bamf-server
```

```sh
# Linux
cargo build --release -p bamf-client
```

### 2. Create a shared key

```powershell
.\target\release\bamf-server.exe keygen
```

### 3. Configure

Copy `config/bamf-server.example.toml` to `bamf-server.toml` (Windows) and
`config/bamf-client.example.toml` to `bamf-client.toml` (Linux). Paste the same
key in both, set the server IP in the client config and the laptop's screen
size and side in the server config.

If the laptop sits higher or lower than the monitor, set `remote_offset`
(pixels from the top of the monitor to the top of the laptop screen) so the
cursor crosses at the matching height.

### 4. Allow access

Windows: allow inbound UDP 24242 (input) and TCP 24242 (clipboard), from an
elevated PowerShell:

```powershell
New-NetFirewallRule -DisplayName "bamf" -Direction Inbound -Protocol UDP -LocalPort 24242 -Action Allow -Profile Private
New-NetFirewallRule -DisplayName "bamf clipboard" -Direction Inbound -Protocol TCP -LocalPort 24242 -Action Allow -Profile Private
```

Linux: let your user create virtual input devices:

```sh
sudo cp packaging/linux/60-bamf-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --name-match=uinput
```

No firewalld rule is needed on Linux: the client talks first, so the replies
are allowed.

### 5. Run

```powershell
.\target\release\bamf-server.exe
```

```sh
./target/release/bamf-client
```

Move the cursor to the configured edge. **Scroll Lock** always brings input
back to Windows. `BAMF_DEBUG=1` enables verbose logs.

### 6. Start automatically

Windows, from a normal PowerShell (copy the program out of `target` so
rebuilding does not lock it):

```powershell
$dir = "$env:USERPROFILE\bamf"; New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item target\release\bamf-server.exe, bamf-server.toml $dir
& "$dir\bamf-server.exe" install "$dir\bamf-server.toml"
```

It then starts at every login without a console window; logs go to
`%LOCALAPPDATA%\bamf\bamf-server.log`. `bamf-server uninstall` undoes it.

Linux, as a systemd user service that starts with the graphical session:

```sh
sh packaging/linux/install-service.sh
journalctl --user -u bamf-client -f    # logs
```

The config moves to `~/.config/bamf/bamf-client.toml`.

### 7. GNOME panel icon and clipboard (Linux)

GNOME does not let background programs read or write the Wayland
clipboard, so clipboard sync on Linux goes through a small GNOME Shell
extension, which also adds the panel icon:

```sh
sh gnome-extension/install.sh
```

Log out and back in once (Wayland only picks up new extensions in a new
session), then `gnome-extensions enable bamf@razimbrao.github.io`.

Without the extension everything else works, and the client can still be
paused from a terminal or a keyboard shortcut:

```sh
./target/release/bamf-client toggle   # or pause, resume, status
```

## Tips

- **Wi-Fi latency**: disable Wi-Fi power saving on the laptop if the cursor
  feels laggy:
  `nmcli connection modify <wifi-name> 802-11-wireless.powersave 2`
- **Admin windows**: Windows does not deliver input to the hooks while an
  elevated window is focused. Run the server as administrator to cover those.
- `Ctrl+Alt+Del` and `Win+L` always go to Windows (secure desktop).
- **Tray icon** (Windows): purple = ready, amber = waiting for the client,
  grey = paused. Right-click for Pause/Resume and Quit.

## Documentation

- [Plan and roadmap](docs/plan.md)
- [Architecture](docs/architecture.md)
- [Wire protocol](docs/protocol.md)
- [Development](docs/development.md)

## License

[MIT](LICENSE)
