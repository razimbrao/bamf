//! Datagram sealing (ChaCha20-Poly1305) and replay protection.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::aead;
use crate::message::{DecodeError, Message};
use crate::PROTOCOL_VERSION;

const HEADER_LEN: usize = 10;
const TAG_LEN: usize = 16;

/// Who sent a datagram. Part of the nonce so both sides never reuse one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    ServerToClient = 0,
    ClientToServer = 1,
}

/// 32-byte pre-shared key.
#[derive(Clone)]
pub struct Key([u8; 32]);

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(hex: &str) -> Result<Self, String> {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.is_ascii() {
            return Err(format!(
                "key must have 64 hex characters, got {}",
                hex.chars().count()
            ));
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| "key contains non-hex characters".to_string())?;
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    TooShort,
    WrongVersion(u8),
    WrongDirection,
    Replayed,
    BadTag,
    Decode(DecodeError),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::TooShort => write!(f, "datagram too short"),
            OpenError::WrongVersion(v) => write!(f, "unsupported protocol version {v}"),
            OpenError::WrongDirection => write!(f, "unexpected direction"),
            OpenError::Replayed => write!(f, "replayed or reordered datagram"),
            OpenError::BadTag => write!(f, "authentication failed (wrong key?)"),
            OpenError::Decode(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// One endpoint of an encrypted channel: seals outgoing and opens incoming
/// datagrams.
pub struct Session {
    key: [u8; 32],
    outgoing: Direction,
    send_counter: u64,
    /// Highest counter accepted from the peer.
    recv_counter: u64,
}

impl Session {
    pub fn new(key: &Key, outgoing: Direction) -> Self {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Self {
            key: key.0,
            outgoing,
            send_counter: now_ns,
            recv_counter: 0,
        }
    }

    fn incoming(&self) -> Direction {
        match self.outgoing {
            Direction::ServerToClient => Direction::ClientToServer,
            Direction::ClientToServer => Direction::ServerToClient,
        }
    }

    fn nonce(direction: Direction, counter: u64) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[0] = direction as u8;
        n[4..].copy_from_slice(&counter.to_le_bytes());
        n
    }

    /// Forgets the peer's replay window. Used by the server when a client
    /// restarts with a lower counter (e.g. its clock moved backwards).
    pub fn reset_replay_window(&mut self) {
        self.recv_counter = 0;
    }

    pub fn seal(&mut self, msg: &Message) -> Vec<u8> {
        self.send_counter += 1;
        let counter = self.send_counter;

        let mut header = [0u8; HEADER_LEN];
        header[0] = PROTOCOL_VERSION;
        header[1] = self.outgoing as u8;
        header[2..].copy_from_slice(&counter.to_le_bytes());

        let mut plain = Vec::with_capacity(40);
        msg.encode(&mut plain);
        let sealed = aead::seal(
            &self.key,
            &Self::nonce(self.outgoing, counter),
            &header,
            &plain,
        );

        let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
        out.extend_from_slice(&header);
        out.extend_from_slice(&sealed);
        out
    }

    pub fn open(&mut self, datagram: &[u8]) -> Result<Message, OpenError> {
        if datagram.len() < HEADER_LEN + TAG_LEN {
            return Err(OpenError::TooShort);
        }
        let (header, sealed) = datagram.split_at(HEADER_LEN);
        if header[0] != PROTOCOL_VERSION {
            return Err(OpenError::WrongVersion(header[0]));
        }
        let incoming = self.incoming();
        if header[1] != incoming as u8 {
            return Err(OpenError::WrongDirection);
        }
        let counter = u64::from_le_bytes(header[2..10].try_into().unwrap());
        if counter <= self.recv_counter {
            return Err(OpenError::Replayed);
        }
        let plain = aead::open(&self.key, &Self::nonce(incoming, counter), header, sealed)
            .ok_or(OpenError::BadTag)?;
        // Only advance after authentication, so forged packets can't block us.
        self.recv_counter = counter;
        Message::decode(&plain).map_err(OpenError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> Key {
        Key::from_bytes([seed; 32])
    }

    fn pair() -> (Session, Session) {
        let k = key(1);
        (
            Session::new(&k, Direction::ServerToClient),
            Session::new(&k, Direction::ClientToServer),
        )
    }

    #[test]
    fn hex_roundtrip() {
        let k = Key::from_bytes(core::array::from_fn(|i| (i * 7) as u8));
        assert_eq!(Key::from_hex(&k.to_hex()).unwrap().0, k.0);
        assert!(Key::from_hex("abc").is_err());
        assert!(Key::from_hex(&"zz".repeat(32)).is_err());
        assert!(Key::from_hex(&"é".repeat(32)).is_err());
    }

    #[test]
    fn seal_and_open() {
        let (mut server, mut client) = pair();
        let msg = Message::MouseRel { dx: 3, dy: -4 };
        let dgram = server.seal(&msg);
        assert_eq!(client.open(&dgram).unwrap(), msg);

        let ping = client.seal(&Message::Ping);
        assert_eq!(server.open(&ping).unwrap(), Message::Ping);
    }

    #[test]
    fn rejects_replay() {
        let (mut server, mut client) = pair();
        let dgram = server.seal(&Message::Leave);
        client.open(&dgram).unwrap();
        assert_eq!(client.open(&dgram), Err(OpenError::Replayed));
    }

    #[test]
    fn rejects_own_direction() {
        let (mut server, _) = pair();
        let mut other_server = Session::new(&key(1), Direction::ServerToClient);
        let dgram = server.seal(&Message::Leave);
        assert_eq!(other_server.open(&dgram), Err(OpenError::WrongDirection));
    }

    #[test]
    fn rejects_tampering_and_wrong_key() {
        let (mut server, mut client) = pair();
        let mut dgram = server.seal(&Message::Leave);
        *dgram.last_mut().unwrap() ^= 1;
        assert_eq!(client.open(&dgram), Err(OpenError::BadTag));

        let mut stranger = Session::new(&key(2), Direction::ClientToServer);
        let dgram = server.seal(&Message::Leave);
        assert_eq!(stranger.open(&dgram), Err(OpenError::BadTag));
        // A forged packet must not advance the replay window.
        assert_eq!(client.open(&dgram).unwrap(), Message::Leave);
    }
}
