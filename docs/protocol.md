# Wire Protocol (v1)

Transport: UDP. The server listens on port `24242` by default. The client sends
from an ephemeral port; the server learns the client address from the last
authenticated packet. Since the client always talks first, a stateful firewall
on the client side (firewalld) needs no extra rule.

All integers are little-endian.

## Datagram

```
+---------+-----------+-------------+----------------------------------+
| version | direction | counter     | ciphertext + 16-byte Poly1305 tag|
| u8 (=3) | u8        | u64         |                                  |
+---------+-----------+-------------+----------------------------------+
 \______________ header (10 bytes, used as AAD) ___/
```

- `direction`: `0` = server → client, `1` = client → server (UDP);
  `2` = server → client, `3` = client → server (TCP clipboard stream).
- Nonce (12 bytes) = `direction` (1 byte) + 3 zero bytes + `counter` (8 bytes).
  Including the direction guarantees that the two sides never reuse a nonce
  with the shared key.
- `counter` starts at the sender's current Unix time in nanoseconds and
  increments per packet. It is therefore strictly increasing, even across
  restarts.
- **Replay protection**: the receiver drops any packet whose counter is not
  greater than the highest counter accepted so far for that direction.
  Reordered packets are dropped too; this is acceptable on a LAN because of
  the key-state resynchronisation below.

## Messages

The plaintext is one message: a `u8` tag followed by its fields.

| Tag | Name | Direction | Fields |
|---|---|---|---|
| `0x01` | `Ping` | C → S | `pause: u8`: `0` no change, `1` pause, `2` resume (the user flipped the switch on the client; repeated until `Status` shows it) |
| `0x02` | `Status` | S → C | `paused: u8`, the shared pause state; reply to every `Ping` |
| `0x10` | `Enter` | S → C | `x: u16`, `y: u16` (absolute position, 0–65535) |
| `0x11` | `Leave` | S → C | — |
| `0x12` | `MouseAbs` | S → C | `x: u16`, `y: u16` |
| `0x13` | `MouseRel` | S → C | `dx: i16`, `dy: i16` |
| `0x14` | `Button` | S → C | `button: u8`, `down: u8` |
| `0x15` | `Wheel` | S → C | `vertical: i16`, `horizontal: i16` (units of 1/120 notch) |
| `0x16` | `Key` | S → C | `seq: u32`, `code: u16` (evdev), `down: u8` |
| `0x17` | `State` | S → C | `keys: [u8; 32]` (bitmap of evdev codes 0–255), `buttons: u8` |

The pause state is shared and lives on the server: its tray icon changes
it directly, the client asks through `Ping`, and every `Status` reports it,
so both machines always show the same state. The cursor only crosses while
it is off; pausing while the cursor is on the client sends it back.
Version 3 replaced version 2's independent per-side flags with this.

## Clipboard stream (TCP)

The clipboard travels on a TCP connection to the same port, opened by the
client. Each frame is a `u32` length followed by one sealed message in the
format above, with the stream directions, so its nonces never collide with
the UDP ones even though both counters start at the current time. Both
sides keep their stream sessions across reconnections, so frames recorded
from an old connection cannot be replayed into a new one. Frames above
48 MiB are refused.

| Tag | Name | Direction | Fields |
|---|---|---|---|
| `0x20` | `Clip` | both | `mime_len: u8`, `mime: [u8]` (UTF-8), `data: [u8]` (rest of the frame) |

`mime` is `text/plain;charset=utf-8` (UTF-8 text, `\n` line endings) or
`image/png`. Each side remembers a fingerprint of the last content it sent
or received and does not send the same content again, so setting the local
clipboard from the peer does not bounce it back.

Buttons: `0` left, `1` right, `2` middle, `3` back (X1), `4` forward (X2).
The `buttons` field of `State` is a bitmask using the same indices.

## Key reliability

UDP may lose packets, and a lost key-release means a stuck key. Three layers
prevent that:

1. Every `Key` message is sent **twice**. The client ignores any `seq` it has
   already applied.
2. While in remote mode, the server sends a `State` snapshot every 200 ms. The
   client compares it with what it has injected and presses/releases keys and
   buttons to match.
3. If the client receives nothing for 1 s while in remote mode, it releases
   everything.
