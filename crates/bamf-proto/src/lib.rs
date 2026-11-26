//! Wire protocol shared by the bamf server and client.
//!
//! See `docs/protocol.md` for the specification.

pub mod aead;
pub mod config;
pub mod crypto;
pub mod keystate;
pub mod log;
pub mod message;

pub use crypto::{Direction, Key, OpenError, Session};
pub use keystate::KeyState;
pub use message::{Button, Message};

/// Default UDP port the server listens on.
pub const DEFAULT_PORT: u16 = 24242;

/// Protocol version carried in every datagram header.
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum value of an absolute coordinate.
pub const ABS_MAX: u16 = u16::MAX;

/// How often the client pings the server.
pub const PING_INTERVAL_MS: u64 = 500;

/// The server considers the client gone after this long without a ping.
pub const CLIENT_TIMEOUT_MS: u64 = 2000;

/// While in remote mode, the server sends a `State` snapshot this often.
pub const STATE_INTERVAL_MS: u64 = 200;

/// The client releases everything after this long without server traffic.
pub const SERVER_TIMEOUT_MS: u64 = 1000;
