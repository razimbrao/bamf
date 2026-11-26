//! Text protocol of the local control socket, used by the GNOME Shell
//! extension and by `bamf-client pause|resume|toggle|status`.
//!
//! One command per line, UTF-8:
//!
//! ```text
//! extension -> client   pause | resume | toggle | status
//!                       clip <mime> <base64>      local clipboard changed
//! client -> extension   status connected=<0|1> remote=<0|1> paused=<0|1> server_paused=<0|1>
//!                       clip <mime> <base64>      set the local clipboard
//! ```
//!
//! Pure logic, no OS calls.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    Toggle,
    Status,
    Clip { mime: String, data: Vec<u8> },
}

pub fn parse(line: &str) -> Result<Command, String> {
    let mut parts = line.trim_end_matches(['\r', '\n']).splitn(3, ' ');
    let cmd = parts.next().unwrap_or_default();
    match cmd {
        "pause" => Ok(Command::Pause),
        "resume" => Ok(Command::Resume),
        "toggle" => Ok(Command::Toggle),
        "status" => Ok(Command::Status),
        "clip" => {
            let mime = parts.next().ok_or("clip: missing mime type")?;
            let data = parts.next().unwrap_or_default();
            if mime.is_empty() || mime.contains(char::is_whitespace) {
                return Err("clip: invalid mime type".into());
            }
            Ok(Command::Clip {
                mime: mime.to_string(),
                data: base64::decode(data).ok_or("clip: invalid base64")?,
            })
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Status {
    /// The server answered recently.
    pub connected: bool,
    /// Keyboard and mouse are currently on this machine.
    pub remote: bool,
    /// Paused here.
    pub paused: bool,
    /// Paused on the server.
    pub server_paused: bool,
}

impl Status {
    pub fn line(&self) -> String {
        format!(
            "status connected={} remote={} paused={} server_paused={}\n",
            self.connected as u8, self.remote as u8, self.paused as u8, self.server_paused as u8
        )
    }
}

pub fn clip_line(mime: &str, data: &[u8]) -> String {
    format!("clip {mime} {}\n", base64::encode(data))
}

/// Standard base64 (RFC 4648) with padding, as produced by
/// `GLib.base64_encode`.
pub mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let text = text.trim_end_matches('=');
        let mut out = Vec::with_capacity(text.len() * 3 / 4);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in text.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            };
            acc = acc << 6 | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        // Leftover bits must be zero padding, and a single extra sextet is
        // never valid.
        (bits < 6 && acc == 0).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64::encode(plain.as_bytes()), enc);
            assert_eq!(base64::decode(enc).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64::decode(&base64::encode(&all)).unwrap(), all);
        assert_eq!(base64::decode("Zm9v!"), None);
        assert_eq!(base64::decode("Z"), None);
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse("pause\n"), Ok(Command::Pause));
        assert_eq!(parse("toggle"), Ok(Command::Toggle));
        assert_eq!(
            parse("clip text/plain;charset=utf-8 b2zDoQ==\n"),
            Ok(Command::Clip {
                mime: "text/plain;charset=utf-8".into(),
                data: "olá".as_bytes().to_vec()
            })
        );
        assert_eq!(
            parse("clip image/png "),
            Ok(Command::Clip {
                mime: "image/png".into(),
                data: vec![]
            })
        );
        assert!(parse("clip").is_err());
        assert!(parse("clip text/plain ***").is_err());
        assert!(parse("explode").is_err());
    }

    #[test]
    fn formats_status_and_clip() {
        let s = Status {
            connected: true,
            paused: true,
            ..Status::default()
        };
        assert_eq!(
            s.line(),
            "status connected=1 remote=0 paused=1 server_paused=0\n"
        );
        assert_eq!(clip_line("text/plain", b"hi"), "clip text/plain aGk=\n");
    }
}
