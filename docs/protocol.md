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
| u8 (=2) | u8        | u64         |                                  |
+---------+-----------+-------------+----------------------------------+
 \______________ header (10 bytes, used as AAD) ___/
```

- `direction`: `0` = server → client, `1` = client → server.
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
| `0x01` | `Ping` | C → S | `paused: u8` (the user paused bamf on the client) |
| `0x02` | `Status` | S → C | `paused: u8` (the user paused bamf on the server); reply to every `Ping` |
| `0x10` | `Enter` | S → C | `x: u16`, `y: u16` (absolute position, 0–65535) |
| `0x11` | `Leave` | S → C | — |
| `0x12` | `MouseAbs` | S → C | `x: u16`, `y: u16` |
| `0x13` | `MouseRel` | S → C | `dx: i16`, `dy: i16` |
| `0x14` | `Button` | S → C | `button: u8`, `down: u8` |
| `0x15` | `Wheel` | S → C | `vertical: i16`, `horizontal: i16` (units of 1/120 notch) |
| `0x16` | `Key` | S → C | `seq: u32`, `code: u16` (evdev), `down: u8` |
| `0x17` | `State` | S → C | `keys: [u8; 32]` (bitmap of evdev codes 0–255), `buttons: u8` |

Version 2 added the pause flags. The cursor only crosses while neither side
is paused; pausing either side while the cursor is on the client sends it
back to the server.

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
