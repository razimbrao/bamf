//! Framing for the TCP stream that carries the clipboard: every frame is a
//! `u32` little-endian length followed by one sealed message (same format
//! as a datagram, with the stream directions).

use std::io::{self, Read, Write};

use crate::crypto::{OpenError, Session};
use crate::message::Message;

/// Largest sealed frame accepted. Clipboard content above roughly this size
/// is not synced.
pub const MAX_FRAME: usize = 48 * 1024 * 1024;

/// Largest clipboard payload worth sending (leaves room for the header).
pub const MAX_CLIP: usize = MAX_FRAME - 1024;

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    TooLarge(usize),
    Open(OpenError),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "{e}"),
            FrameError::TooLarge(n) => write!(f, "frame of {n} bytes is too large"),
            FrameError::Open(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        FrameError::Io(e)
    }
}

pub fn write_frame<W: Write>(w: &mut W, session: &mut Session, msg: &Message) -> io::Result<()> {
    let sealed = session.seal(msg);
    w.write_all(&(sealed.len() as u32).to_le_bytes())?;
    w.write_all(&sealed)?;
    w.flush()
}

/// Reads one frame. Any error means the stream must be dropped: a frame
/// that fails authentication leaves the reader out of sync.
pub fn read_frame<R: Read>(r: &mut R, session: &mut Session) -> Result<Message, FrameError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    session.open(&buf).map_err(FrameError::Open)
}

/// Remembers the last clipboard content seen on either side, so content
/// that just arrived is not sent straight back when the local clipboard
/// reports the change we made ourselves.
#[derive(Debug, Default)]
pub struct EchoGuard {
    last: Option<u64>,
}

impl EchoGuard {
    /// Records content received from the peer.
    pub fn received(&mut self, mime: &str, data: &[u8]) {
        self.last = Some(fingerprint(mime, data));
    }

    /// True when locally changed content should go to the peer.
    pub fn should_send(&mut self, mime: &str, data: &[u8]) -> bool {
        let fp = fingerprint(mime, data);
        if self.last == Some(fp) {
            return false;
        }
        self.last = Some(fp);
        true
    }
}

/// FNV-1a over the MIME type and data. Not cryptographic; only compares
/// local clipboard contents.
fn fingerprint(mime: &str, data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in mime.as_bytes().iter().chain([0u8].iter()).chain(data) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{Direction, Key};
    use crate::message::MIME_TEXT;

    #[test]
    fn frames_roundtrip_and_reject_garbage() {
        let key = Key::from_bytes([7; 32]);
        let mut server = Session::new(&key, Direction::ServerToClientStream);
        let mut client = Session::new(&key, Direction::ClientToServerStream);
        let msg = Message::Clip {
            mime: MIME_TEXT.into(),
            data: b"hello".to_vec(),
        };

        let mut wire = Vec::new();
        write_frame(&mut wire, &mut server, &msg).unwrap();
        write_frame(&mut wire, &mut server, &msg).unwrap();
        let mut r = &wire[..];
        assert_eq!(read_frame(&mut r, &mut client).unwrap(), msg);
        assert_eq!(read_frame(&mut r, &mut client).unwrap(), msg);
        assert!(matches!(
            read_frame(&mut r, &mut client),
            Err(FrameError::Io(_))
        ));

        // Too large is refused before allocating.
        let huge = ((MAX_FRAME + 1) as u32).to_le_bytes();
        assert!(matches!(
            read_frame(&mut &huge[..], &mut client),
            Err(FrameError::TooLarge(_))
        ));

        // A UDP session cannot open stream frames (different direction).
        let mut udp = Session::new(&key, Direction::ClientToServer);
        let mut wire = Vec::new();
        write_frame(&mut wire, &mut server, &msg).unwrap();
        assert!(matches!(
            read_frame(&mut &wire[..], &mut udp),
            Err(FrameError::Open(OpenError::WrongDirection))
        ));
    }

    #[test]
    fn echo_guard_drops_what_just_arrived() {
        let mut g = EchoGuard::default();
        g.received(MIME_TEXT, b"from peer");
        assert!(!g.should_send(MIME_TEXT, b"from peer"));
        assert!(g.should_send(MIME_TEXT, b"typed here"));
        assert!(!g.should_send(MIME_TEXT, b"typed here")); // reported twice
        assert!(g.should_send("image/png", b"typed here"));
    }
}
