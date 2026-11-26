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

MVP: one-way control Windows → Linux. Keyboard (including ABNT2 keys),
mouse movement, five buttons and both wheels, edge switching, encryption,
stuck-key protection, and a panic key. See [docs/plan.md](docs/plan.md) for
the roadmap.

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

### 4. Allow access

Windows: allow inbound UDP 24242 (Windows asks on first run, or):

```powershell
New-NetFirewallRule -DisplayName "bamf" -Direction Inbound -Protocol UDP -LocalPort 24242 -Action Allow
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

## Tips

- **Wi-Fi latency**: disable Wi-Fi power saving on the laptop if the cursor
  feels laggy:
  `nmcli connection modify <wifi-name> 802-11-wireless.powersave 2`
- **Admin windows**: Windows does not deliver input to the hooks while an
  elevated window is focused. Run the server as administrator to cover those.
- `Ctrl+Alt+Del` and `Win+L` always go to Windows (secure desktop).

## Documentation

- [Plan and roadmap](docs/plan.md)
- [Architecture](docs/architecture.md)
- [Wire protocol](docs/protocol.md)
- [Development](docs/development.md)

## License

[MIT](LICENSE)
