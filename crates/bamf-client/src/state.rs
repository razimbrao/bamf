//! Turns protocol messages into input actions, keeping track of what is
//! pressed so nothing gets stuck. Pure logic, no OS calls.

use bamf_proto::{Button, KeyState, Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Key { code: u16, down: bool },
    Button { button: Button, down: bool },
    MoveAbs { x: u16, y: u16 },
    MoveRel { dx: i16, dy: i16 },
    Wheel { vertical: i16, horizontal: i16 },
}

#[derive(Debug, Default)]
pub struct ClientState {
    active: bool,
    last_seq: Option<u32>,
    keys: KeyState,
    buttons: u8,
    server_paused: bool,
}

impl ClientState {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_server_paused(&self) -> bool {
        self.server_paused
    }

    pub fn handle(&mut self, msg: Message, out: &mut Vec<Action>) {
        match msg {
            Message::Ping { .. } => {}
            Message::Status { paused } => self.server_paused = paused,
            // Travels on the TCP stream, never as a datagram.
            Message::Clip { .. } => {}
            Message::Enter { x, y } => {
                self.active = true;
                self.last_seq = None;
                out.push(Action::MoveAbs { x, y });
            }
            Message::Leave => {
                self.release_all(out);
                self.active = false;
            }
            Message::MouseAbs { x, y } => {
                self.active = true;
                out.push(Action::MoveAbs { x, y });
            }
            Message::MouseRel { dx, dy } => {
                self.active = true;
                out.push(Action::MoveRel { dx, dy });
            }
            Message::Button { button, down } => {
                self.active = true;
                self.set_button(button, down, out);
            }
            Message::Wheel {
                vertical,
                horizontal,
            } => {
                self.active = true;
                out.push(Action::Wheel {
                    vertical,
                    horizontal,
                });
            }
            Message::Key { seq, code, down } => {
                self.active = true;
                // Keys are sent twice; apply each sequence number once.
                // Wrapping comparison tolerates the u32 counter overflowing.
                if let Some(last) = self.last_seq {
                    if (seq.wrapping_sub(last) as i32) <= 0 {
                        return;
                    }
                }
                self.last_seq = Some(seq);
                self.set_key(code, down, out);
            }
            Message::State { keys, buttons } => {
                self.active = true;
                self.reconcile(&keys, buttons, out);
            }
        }
    }

    /// Server went silent: release everything that is still pressed.
    pub fn timeout(&mut self, out: &mut Vec<Action>) {
        self.release_all(out);
        self.active = false;
    }

    fn set_key(&mut self, code: u16, down: bool, out: &mut Vec<Action>) {
        if self.keys.is_down(code) != down {
            self.keys.set(code, down);
            out.push(Action::Key { code, down });
        }
    }

    fn set_button(&mut self, button: Button, down: bool, out: &mut Vec<Action>) {
        let is_down = self.buttons & button.mask() != 0;
        if is_down != down {
            if down {
                self.buttons |= button.mask();
            } else {
                self.buttons &= !button.mask();
            }
            out.push(Action::Button { button, down });
        }
    }

    fn reconcile(&mut self, keys: &KeyState, buttons: u8, out: &mut Vec<Action>) {
        let pressed: Vec<u16> = self.keys.pressed().collect();
        for code in pressed {
            if !keys.is_down(code) {
                self.set_key(code, false, out);
            }
        }
        for code in keys.pressed() {
            self.set_key(code, true, out);
        }
        for button in Button::ALL {
            self.set_button(button, buttons & button.mask() != 0, out);
        }
    }

    fn release_all(&mut self, out: &mut Vec<Action>) {
        self.reconcile(&KeyState::default(), 0, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(state: &mut ClientState, msgs: Vec<Message>) -> Vec<Action> {
        let mut out = Vec::new();
        for m in msgs {
            state.handle(m, &mut out);
        }
        out
    }

    fn key(seq: u32, code: u16, down: bool) -> Message {
        Message::Key { seq, code, down }
    }

    #[test]
    fn duplicated_keys_apply_once() {
        let mut s = ClientState::default();
        let out = run(
            &mut s,
            vec![
                Message::Enter { x: 0, y: 10 },
                key(1, 30, true),
                key(1, 30, true),
                key(2, 30, false),
                key(2, 30, false),
            ],
        );
        assert_eq!(
            out,
            vec![
                Action::MoveAbs { x: 0, y: 10 },
                Action::Key {
                    code: 30,
                    down: true
                },
                Action::Key {
                    code: 30,
                    down: false
                },
            ]
        );
    }

    #[test]
    fn state_snapshot_fixes_lost_release() {
        let mut s = ClientState::default();
        run(&mut s, vec![key(1, 42, true)]); // Shift down; its release is lost
        let mut keys = KeyState::default();
        keys.set(30, true);
        let out = run(
            &mut s,
            vec![Message::State {
                keys,
                buttons: Button::Left.mask(),
            }],
        );
        assert_eq!(
            out,
            vec![
                Action::Key {
                    code: 42,
                    down: false
                },
                Action::Key {
                    code: 30,
                    down: true
                },
                Action::Button {
                    button: Button::Left,
                    down: true
                },
            ]
        );
    }

    #[test]
    fn leave_and_timeout_release_everything() {
        let mut s = ClientState::default();
        run(
            &mut s,
            vec![
                key(1, 29, true),
                Message::Button {
                    button: Button::Right,
                    down: true,
                },
            ],
        );
        let out = run(&mut s, vec![Message::Leave]);
        assert_eq!(out.len(), 2);
        assert!(!s.is_active());

        run(&mut s, vec![key(2, 56, true)]);
        assert!(s.is_active());
        let mut out = Vec::new();
        s.timeout(&mut out);
        assert_eq!(
            out,
            vec![Action::Key {
                code: 56,
                down: false
            }]
        );
        assert!(!s.is_active());
    }

    #[test]
    fn enter_resets_sequence_and_seq_wraps() {
        let mut s = ClientState::default();
        run(&mut s, vec![key(u32::MAX, 30, true)]);
        // Wrapped counter is still "newer".
        let out = run(&mut s, vec![key(0, 30, false)]);
        assert_eq!(
            out,
            vec![Action::Key {
                code: 30,
                down: false
            }]
        );

        // A restarted server begins at 1 again; Enter makes that acceptable.
        let out = run(
            &mut s,
            vec![Message::Enter { x: 0, y: 0 }, key(1, 31, true)],
        );
        assert_eq!(
            out.last(),
            Some(&Action::Key {
                code: 31,
                down: true
            })
        );
    }
}
