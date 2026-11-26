# Development

## Layout

```
crates/
  bamf-proto/    protocol, crypto (RFC 8439), config parser, logger
  bamf-server/   Windows server (core logic + windows/ FFI glue)
  bamf-client/   Linux client (state logic + linux/ uinput glue)
config/          example config files
packaging/linux/ udev rule for /dev/uinput
scripts/         test helpers
docs/            this documentation
```

Platform-specific code lives in `windows/` and `linux/` modules. Everything
else is plain logic behind small seams (`Platform` trait, action lists), so it
compiles and is unit-tested on every OS.

## Zero dependencies

The workspace uses only the Rust standard library:

- Win32 and uinput are reached through hand-written FFI declarations
  (`crates/bamf-server/src/windows/ffi.rs`, `crates/bamf-client/src/linux/uinput.rs`).
- ChaCha20-Poly1305 is implemented in `crates/bamf-proto/src/aead.rs` and
  validated against the RFC 8439 test vectors plus an independent big-integer
  reference (`scripts/poly1305_reference.py`).
- Config files use a tiny `key = value` parser.

Why: it is a study project (seeing how these pieces work is the point), and
the development machine runs Windows **Smart App Control**, which blocks some
freshly built executables, including the build scripts of common crates such
as `generic-array`. With no build scripts, the workspace always compiles.

The trade-off is a hand-rolled AEAD. It passes the official vectors, but it has
not been audited. Swapping it for the `chacha20poly1305` crate only touches
`aead::seal` / `aead::open`.

## Building

```sh
cargo build --release -p bamf-server   # on Windows
cargo build --release -p bamf-client   # on Linux
```

Cross-check the other platform without its linker:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu
```

## Testing

```sh
cargo test --workspace
```

If Smart App Control blocks the native test binaries (`os error 4551`), run the
tests as WebAssembly through Node.js instead (Node is signed, so it is allowed
to run):

```sh
rustup target add wasm32-wasip1
cargo test --workspace --target wasm32-wasip1
```

`.cargo/config.toml` sets `scripts/wasi-runner.mjs` as the runner for that
target. The UDP loopback test is skipped there (WASI has no sockets).

CI (`.github/workflows/ci.yml`) runs fmt, clippy and the tests natively on
Ubuntu and Windows.

## Conventions

- Code, comments and docs in English.
- [Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
  `fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `ci:`.
- `cargo fmt` and `cargo clippy` clean before committing.
