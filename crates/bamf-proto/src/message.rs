//! Plaintext messages and their binary encoding.

use crate::keystate::KeyState;

/// Mouse buttons. The discriminant is the wire value and the bit index used
/// in [`Message::State`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    Left = 0,
    Right = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

impl Button {
    pub const ALL: [Button; 5] = [
        Button::Left,
        Button::Right,
        Button::Middle,
        Button::Back,
        Button::Forward,
    ];

    pub fn from_u8(value: u8) -> Option<Self> {
        Self::ALL.get(value as usize).copied()
    }

    pub fn mask(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Client liveness signal.
    Ping,
    /// The cursor entered the client screen at an absolute position.
    Enter {
        x: u16,
        y: u16,
    },
    /// The cursor went back to the server.
    Leave,
    MouseAbs {
        x: u16,
        y: u16,
    },
    MouseRel {
        dx: i16,
        dy: i16,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// Wheel movement in units of 1/120 of a notch.
    Wheel {
        vertical: i16,
        horizontal: i16,
    },
    Key {
        seq: u32,
        code: u16,
        down: bool,
    },
    /// Snapshot of everything currently pressed on the remote side.
    State {
        keys: KeyState,
        buttons: u8,
    },
}

mod tag {
    pub const PING: u8 = 0x01;
    pub const ENTER: u8 = 0x10;
    pub const LEAVE: u8 = 0x11;
    pub const MOUSE_ABS: u8 = 0x12;
    pub const MOUSE_REL: u8 = 0x13;
    pub const BUTTON: u8 = 0x14;
    pub const WHEEL: u8 = 0x15;
    pub const KEY: u8 = 0x16;
    pub const STATE: u8 = 0x17;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    UnknownTag(u8),
    BadLength { tag: u8, len: usize },
    BadValue,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Empty => write!(f, "empty message"),
            DecodeError::UnknownTag(t) => write!(f, "unknown message tag 0x{t:02x}"),
            DecodeError::BadLength { tag, len } => {
                write!(f, "bad length {len} for message tag 0x{tag:02x}")
            }
            DecodeError::BadValue => write!(f, "invalid field value"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl Message {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Message::Ping => out.push(tag::PING),
            Message::Enter { x, y } => {
                out.push(tag::ENTER);
                out.extend_from_slice(&x.to_le_bytes());
                out.extend_from_slice(&y.to_le_bytes());
            }
            Message::Leave => out.push(tag::LEAVE),
            Message::MouseAbs { x, y } => {
                out.push(tag::MOUSE_ABS);
                out.extend_from_slice(&x.to_le_bytes());
                out.extend_from_slice(&y.to_le_bytes());
            }
            Message::MouseRel { dx, dy } => {
                out.push(tag::MOUSE_REL);
                out.extend_from_slice(&dx.to_le_bytes());
                out.extend_from_slice(&dy.to_le_bytes());
            }
            Message::Button { button, down } => {
                out.extend_from_slice(&[tag::BUTTON, *button as u8, *down as u8]);
            }
            Message::Wheel {
                vertical,
                horizontal,
            } => {
                out.push(tag::WHEEL);
                out.extend_from_slice(&vertical.to_le_bytes());
                out.extend_from_slice(&horizontal.to_le_bytes());
            }
            Message::Key { seq, code, down } => {
                out.push(tag::KEY);
                out.extend_from_slice(&seq.to_le_bytes());
                out.extend_from_slice(&code.to_le_bytes());
                out.push(*down as u8);
            }
            Message::State { keys, buttons } => {
                out.push(tag::STATE);
                out.extend_from_slice(keys.as_bytes());
                out.push(*buttons);
            }
        }
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let (&tag, body) = buf.split_first().ok_or(DecodeError::Empty)?;
        let expect = |len: usize| {
            if body.len() == len {
                Ok(())
            } else {
                Err(DecodeError::BadLength {
                    tag,
                    len: body.len(),
                })
            }
        };
        let u16_at = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
        let i16_at = |i: usize| i16::from_le_bytes([body[i], body[i + 1]]);
        let bool_at = |i: usize| match body[i] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::BadValue),
        };

        let msg = match tag {
            tag::PING => {
                expect(0)?;
                Message::Ping
            }
            tag::ENTER => {
                expect(4)?;
                Message::Enter {
                    x: u16_at(0),
                    y: u16_at(2),
                }
            }
            tag::LEAVE => {
                expect(0)?;
                Message::Leave
            }
            tag::MOUSE_ABS => {
                expect(4)?;
                Message::MouseAbs {
                    x: u16_at(0),
                    y: u16_at(2),
                }
            }
            tag::MOUSE_REL => {
                expect(4)?;
                Message::MouseRel {
                    dx: i16_at(0),
                    dy: i16_at(2),
                }
            }
            tag::BUTTON => {
                expect(2)?;
                Message::Button {
                    button: Button::from_u8(body[0]).ok_or(DecodeError::BadValue)?,
                    down: bool_at(1)?,
                }
            }
            tag::WHEEL => {
                expect(4)?;
                Message::Wheel {
                    vertical: i16_at(0),
                    horizontal: i16_at(2),
                }
            }
            tag::KEY => {
                expect(7)?;
                Message::Key {
                    seq: u32::from_le_bytes([body[0], body[1], body[2], body[3]]),
                    code: u16_at(4),
                    down: bool_at(6)?,
                }
            }
            tag::STATE => {
                expect(KeyState::BYTES + 1)?;
                let mut bytes = [0u8; KeyState::BYTES];
                bytes.copy_from_slice(&body[..KeyState::BYTES]);
                Message::State {
                    keys: KeyState::from_bytes(bytes),
                    buttons: body[KeyState::BYTES],
                }
            }
            other => return Err(DecodeError::UnknownTag(other)),
        };
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: Message) {
        let mut buf = Vec::new();
        msg.encode(&mut buf);
        assert_eq!(Message::decode(&buf).unwrap(), msg);
    }

    #[test]
    fn all_messages_roundtrip() {
        let mut keys = KeyState::default();
        keys.set(30, true);
        keys.set(255, true);
        for msg in [
            Message::Ping,
            Message::Enter { x: 0, y: 65535 },
            Message::Leave,
            Message::MouseAbs { x: 123, y: 456 },
            Message::MouseRel { dx: -5, dy: 300 },
            Message::Button {
                button: Button::Forward,
                down: true,
            },
            Message::Wheel {
                vertical: -120,
                horizontal: 240,
            },
            Message::Key {
                seq: 42,
                code: 30,
                down: false,
            },
            Message::State {
                keys,
                buttons: 0b101,
            },
        ] {
            roundtrip(msg);
        }
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(Message::decode(&[]), Err(DecodeError::Empty));
        assert_eq!(Message::decode(&[0xff]), Err(DecodeError::UnknownTag(0xff)));
        assert!(Message::decode(&[0x10, 1]).is_err());
        assert_eq!(Message::decode(&[0x14, 9, 1]), Err(DecodeError::BadValue));
        assert_eq!(Message::decode(&[0x14, 0, 2]), Err(DecodeError::BadValue));
    }
}
