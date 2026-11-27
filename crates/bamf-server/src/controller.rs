//! The switching state machine. Decides, for every captured input event,
//! whether Windows should see it (`Verdict::Pass`) or whether it goes to the
//! client (`Verdict::Block` plus queued messages).
//!
//! OS-specific side effects (cursor hiding and warping) go through the
//! [`Platform`] trait so this module can be unit-tested anywhere.

use std::time::{Duration, Instant};

use bamf_proto::{Button, KeyState, Message, CLIENT_TIMEOUT_MS};

use crate::geometry::{Geometry, Motion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerMode {
    Absolute,
    Relative,
}

impl std::str::FromStr for PointerMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "absolute" => Ok(PointerMode::Absolute),
            "relative" => Ok(PointerMode::Relative),
            other => Err(format!(
                "invalid pointer mode `{other}` (absolute, relative)"
            )),
        }
    }
}

pub trait Platform {
    fn hide_cursor(&mut self);
    fn show_cursor(&mut self);
    fn warp_cursor(&mut self, x: i32, y: i32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    /// Position the cursor would move to.
    Move {
        x: i32,
        y: i32,
    },
    Button {
        button: Button,
        down: bool,
    },
    Wheel {
        vertical: i16,
        horizontal: i16,
    },
}

/// Deltas larger than this right after parking the cursor are stale events
/// computed from the old position; they are dropped.
const MAX_SANE_DELTA: i32 = 500;

fn rel_message(dx: i32, dy: i32) -> Message {
    let clamp = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    Message::MouseRel {
        dx: clamp(dx),
        dy: clamp(dy),
    }
}

pub struct Controller<P: Platform> {
    pub platform: P,
    geometry: Geometry,
    mode: PointerMode,
    /// Where the real cursor is parked while in remote mode.
    park: (i32, i32),
    /// Virtual cursor on the remote screen; `Some` while in remote mode.
    remote: Option<(f64, f64)>,
    remote_keys: KeyState,
    remote_buttons: u8,
    local_buttons: u8,
    key_seq: u32,
    last_ping: Option<Instant>,
    /// Shared pause state: the tray icon here and the client both change it,
    /// and every ping reply reports it back to the client.
    paused: bool,
    /// Messages to send to the client, drained by the caller.
    pub outbox: Vec<Message>,
}

impl<P: Platform> Controller<P> {
    pub fn new(platform: P, geometry: Geometry, mode: PointerMode, park: (i32, i32)) -> Self {
        Self {
            platform,
            geometry,
            mode,
            park,
            remote: None,
            remote_keys: KeyState::default(),
            remote_buttons: 0,
            local_buttons: 0,
            key_seq: 0,
            last_ping: None,
            paused: false,
            outbox: Vec::new(),
        }
    }

    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    pub fn client_alive(&self, now: Instant) -> bool {
        self.last_ping
            .is_some_and(|t| now.duration_since(t) < Duration::from_millis(CLIENT_TIMEOUT_MS))
    }

    /// True when the cursor may cross: the client answers and bamf is not
    /// paused.
    pub fn can_cross(&self, now: Instant) -> bool {
        self.client_alive(now) && !self.paused
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Records a client ping, applying the pause switch the user flipped
    /// there, if any; returns the reply with the shared state.
    pub fn on_ping(&mut self, now: Instant, pause: Option<bool>) -> Message {
        self.last_ping = Some(now);
        if let Some(paused) = pause {
            if paused != self.paused {
                bamf_proto::info!(
                    "{} from the client",
                    if paused { "paused" } else { "resumed" }
                );
            }
            self.set_paused(paused);
        }
        Message::Status {
            paused: self.paused,
        }
    }

    /// Pauses or resumes bamf for both machines. Pausing takes input back.
    pub fn set_paused(&mut self, paused: bool) {
        if paused != self.paused {
            bamf_proto::info!("{}", if paused { "paused" } else { "resumed" });
            self.paused = paused;
            if paused {
                self.force_local();
            }
        }
    }

    /// Periodic housekeeping: leave remote mode if the client vanished,
    /// otherwise send a state snapshot.
    pub fn tick(&mut self, now: Instant) {
        if !self.is_remote() {
            return;
        }
        if !self.client_alive(now) {
            bamf_proto::warn!("client stopped responding, taking input back");
            let local = self.fallback_exit_point();
            self.leave(local);
            return;
        }
        self.outbox.push(Message::State {
            keys: self.remote_keys,
            buttons: self.remote_buttons,
        });
    }

    pub fn geometry(&self) -> Geometry {
        self.geometry
    }

    /// New screen arrangement (from the settings window). Input comes back
    /// to Windows first if it was on the client.
    pub fn set_geometry(&mut self, geometry: Geometry) {
        self.force_local();
        self.geometry = geometry;
    }

    /// Panic button / shutdown: return to local mode immediately.
    pub fn force_local(&mut self) {
        if self.is_remote() {
            let local = self.fallback_exit_point();
            self.leave(local);
        }
    }

    fn fallback_exit_point(&self) -> (i32, i32) {
        self.park
    }

    fn enter(&mut self, x: i32, y: i32) {
        let pos = self.geometry.entry_point(x, y);
        let (ax, ay) = self.geometry.normalise(pos);
        self.remote = Some(pos);
        self.remote_keys.clear();
        self.remote_buttons = 0;
        self.platform.hide_cursor();
        self.platform.warp_cursor(self.park.0, self.park.1);
        self.outbox.push(Message::Enter { x: ax, y: ay });
        bamf_proto::info!("bamf! -> remote");
    }

    fn leave(&mut self, local: (i32, i32)) {
        self.remote = None;
        self.remote_keys.clear();
        self.remote_buttons = 0;
        self.outbox.push(Message::Leave);
        self.platform.show_cursor();
        self.platform.warp_cursor(local.0, local.1);
        bamf_proto::info!("bamf! <- local");
    }

    pub fn on_mouse(&mut self, event: MouseEvent, now: Instant) -> Verdict {
        let Some(mut pos) = self.remote else {
            return self.on_mouse_local(event, now);
        };

        match event {
            MouseEvent::Move { x, y } => {
                let (dx, dy) = (x - self.park.0, y - self.park.1);
                if (dx, dy) == (0, 0) || dx.abs() > MAX_SANE_DELTA || dy.abs() > MAX_SANE_DELTA {
                    return Verdict::Block;
                }
                match self.geometry.apply(&mut pos, dx, dy) {
                    Motion::Remote { abs, rel, push } => {
                        self.remote = Some(pos);
                        match self.mode {
                            PointerMode::Absolute => {
                                self.outbox.push(Message::MouseAbs { x: abs.0, y: abs.1 });
                                // An absolute position cannot express pressing
                                // against an edge; send that part as a delta
                                // so pointer barriers (docks, hot corners) work.
                                if push != (0, 0) {
                                    self.outbox.push(rel_message(push.0, push.1));
                                }
                            }
                            PointerMode::Relative => self
                                .outbox
                                .push(rel_message(rel.0 + push.0, rel.1 + push.1)),
                        }
                    }
                    Motion::Leave { local } => self.leave(local),
                }
                Verdict::Block
            }
            MouseEvent::Button { button, down } => {
                let mask = button.mask();
                if !down && self.remote_buttons & mask == 0 {
                    // Pressed before crossing: let Windows see the release.
                    self.local_buttons &= !mask;
                    return Verdict::Pass;
                }
                if down {
                    self.remote_buttons |= mask;
                } else {
                    self.remote_buttons &= !mask;
                }
                self.outbox.push(Message::Button { button, down });
                Verdict::Block
            }
            MouseEvent::Wheel {
                vertical,
                horizontal,
            } => {
                self.outbox.push(Message::Wheel {
                    vertical,
                    horizontal,
                });
                Verdict::Block
            }
        }
    }

    fn on_mouse_local(&mut self, event: MouseEvent, now: Instant) -> Verdict {
        match event {
            MouseEvent::Move { x, y } => {
                // Never cross while dragging something.
                if self.local_buttons == 0 && self.geometry.at_edge(x, y) && self.can_cross(now) {
                    self.enter(x, y);
                    return Verdict::Block;
                }
            }
            MouseEvent::Button { button, down } => {
                if down {
                    self.local_buttons |= button.mask();
                } else {
                    self.local_buttons &= !button.mask();
                }
            }
            MouseEvent::Wheel { .. } => {}
        }
        Verdict::Pass
    }

    /// `code` is the evdev key code (`None` if unmapped). `panic_key` is true
    /// for the key that forces local mode (Scroll Lock).
    pub fn on_key(&mut self, code: Option<u16>, down: bool, panic_key: bool) -> Verdict {
        if !self.is_remote() {
            return Verdict::Pass;
        }
        if panic_key {
            if down {
                self.force_local();
            }
            return Verdict::Block;
        }
        let Some(code) = code else {
            bamf_proto::debug!("unmapped key ignored");
            return Verdict::Block;
        };
        if !down && !self.remote_keys.is_down(code) {
            // Pressed before crossing (e.g. Ctrl): let Windows see the release.
            return Verdict::Pass;
        }
        self.remote_keys.set(code, down);
        self.key_seq = self.key_seq.wrapping_add(1);
        let msg = Message::Key {
            seq: self.key_seq,
            code,
            down,
        };
        // Sent twice: a lost key event is much worse than a lost mouse move.
        self.outbox.push(msg.clone());
        self.outbox.push(msg);
        Verdict::Block
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Rect, Side};
    use bamf_proto::ABS_MAX;

    #[derive(Default)]
    struct FakePlatform {
        hidden: bool,
        cursor: (i32, i32),
    }

    impl Platform for FakePlatform {
        fn hide_cursor(&mut self) {
            self.hidden = true;
        }
        fn show_cursor(&mut self) {
            self.hidden = false;
        }
        fn warp_cursor(&mut self, x: i32, y: i32) {
            self.cursor = (x, y);
        }
    }

    const PARK: (i32, i32) = (960, 540);

    fn controller(mode: PointerMode) -> (Controller<FakePlatform>, Instant) {
        let geometry = Geometry {
            local: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            remote_width: 1920,
            remote_height: 1080,
            side: Side::Right,
            offset: None,
        };
        let mut c = Controller::new(FakePlatform::default(), geometry, mode, PARK);
        let now = Instant::now();
        c.on_ping(now, None);
        (c, now)
    }

    fn mv(x: i32, y: i32) -> MouseEvent {
        MouseEvent::Move { x, y }
    }

    fn enter(c: &mut Controller<FakePlatform>, now: Instant) {
        assert_eq!(c.on_mouse(mv(1919, 540), now), Verdict::Block);
        assert!(c.is_remote());
        c.outbox.clear();
    }

    #[test]
    fn crosses_at_edge_only_when_client_alive() {
        let (mut c, now) = controller(PointerMode::Absolute);
        assert_eq!(c.on_mouse(mv(1000, 500), now), Verdict::Pass);
        assert!(!c.is_remote());

        let later = now + Duration::from_millis(CLIENT_TIMEOUT_MS + 1);
        assert_eq!(c.on_mouse(mv(1919, 500), later), Verdict::Pass);
        assert!(!c.is_remote());

        assert_eq!(c.on_mouse(mv(1919, 500), now), Verdict::Block);
        assert!(c.is_remote());
        assert!(c.platform.hidden);
        assert_eq!(c.platform.cursor, PARK);
        assert!(matches!(c.outbox[0], Message::Enter { x: 0, .. }));
    }

    #[test]
    fn pause_is_shared_and_either_side_can_flip_it() {
        let (mut c, now) = controller(PointerMode::Absolute);
        // Paused here (tray): blocks crossing and is reported to the client.
        c.set_paused(true);
        assert_eq!(c.on_mouse(mv(1919, 500), now), Verdict::Pass);
        assert_eq!(c.on_ping(now, None), Message::Status { paused: true });

        // Resumed from the client.
        assert_eq!(
            c.on_ping(now, Some(false)),
            Message::Status { paused: false }
        );
        enter(&mut c, now);

        // Paused from the client while the cursor is over there.
        assert_eq!(c.on_ping(now, Some(true)), Message::Status { paused: true });
        assert!(!c.is_remote());
        assert_eq!(c.outbox, vec![Message::Leave]);
        c.outbox.clear();
        assert!(c.is_paused());
        assert_eq!(c.on_mouse(mv(1919, 500), now), Verdict::Pass);

        // Plain pings change nothing; resuming here works again.
        c.on_ping(now, None);
        assert!(c.is_paused());
        c.set_paused(false);
        assert_eq!(c.on_mouse(mv(1919, 500), now), Verdict::Block);
        c.set_paused(true);
        assert!(!c.is_remote());
    }

    #[test]
    fn does_not_cross_while_dragging() {
        let (mut c, now) = controller(PointerMode::Absolute);
        let left = |down| MouseEvent::Button {
            button: Button::Left,
            down,
        };
        c.on_mouse(left(true), now);
        assert_eq!(c.on_mouse(mv(1919, 500), now), Verdict::Pass);
        c.on_mouse(left(false), now);
        assert_eq!(c.on_mouse(mv(1919, 500), now), Verdict::Block);
    }

    #[test]
    fn moves_relative_to_park_and_comes_back() {
        let (mut c, now) = controller(PointerMode::Relative);
        enter(&mut c, now);
        assert_eq!(c.on_mouse(mv(PARK.0 + 10, PARK.1 - 3), now), Verdict::Block);
        assert_eq!(c.outbox, vec![Message::MouseRel { dx: 10, dy: -3 }]);
        c.outbox.clear();

        // Stale event computed from the pre-park position is dropped.
        c.on_mouse(mv(1919, 540), now);
        assert!(c.outbox.is_empty());

        c.on_mouse(mv(PARK.0 - 50, PARK.1), now);
        assert!(!c.is_remote());
        assert!(!c.platform.hidden);
        assert_eq!(c.outbox, vec![Message::Leave]);
        assert_eq!(c.platform.cursor.0, 1918);
    }

    #[test]
    fn edge_pressure_is_sent_as_relative_motion() {
        let (mut c, now) = controller(PointerMode::Absolute);
        enter(&mut c, now);
        // Inside the screen: absolute position only.
        c.on_mouse(mv(PARK.0 + 5, PARK.1), now);
        assert!(matches!(c.outbox[..], [Message::MouseAbs { .. }]));
        c.outbox.clear();

        // Down to the bottom edge and beyond: the overflow becomes a delta.
        c.on_mouse(mv(PARK.0, PARK.1 + 400), now);
        c.outbox.clear();
        c.on_mouse(mv(PARK.0, PARK.1 + 400), now);
        c.on_mouse(mv(PARK.0, PARK.1 + 400), now);
        // 940 -> 1079 (clamped, 261 lost), then fully at the edge (400 lost).
        assert!(matches!(
            c.outbox[..],
            [
                Message::MouseAbs { y: ABS_MAX, .. },
                Message::MouseRel { dx: 0, dy: 261 },
                Message::MouseAbs { y: ABS_MAX, .. },
                Message::MouseRel { dx: 0, dy: 400 },
            ]
        ));

        // Relative mode sends the raw delta even at the edge.
        let (mut c, now) = controller(PointerMode::Relative);
        enter(&mut c, now);
        c.on_mouse(mv(PARK.0, PARK.1 + 400), now);
        c.on_mouse(mv(PARK.0, PARK.1 + 400), now);
        c.on_mouse(mv(PARK.0, PARK.1 + 400), now);
        assert_eq!(c.outbox[2], Message::MouseRel { dx: 0, dy: 400 });
    }

    #[test]
    fn keys_are_forwarded_twice_and_releases_pass_through() {
        let (mut c, now) = controller(PointerMode::Absolute);
        assert_eq!(c.on_key(Some(30), true, false), Verdict::Pass); // local mode
        enter(&mut c, now);

        assert_eq!(c.on_key(Some(30), false, false), Verdict::Pass); // held before
        assert_eq!(c.on_key(Some(31), true, false), Verdict::Block);
        let key = Message::Key {
            seq: 1,
            code: 31,
            down: true,
        };
        assert_eq!(c.outbox, vec![key.clone(), key]);

        c.outbox.clear();
        c.tick(now);
        match &c.outbox[0] {
            Message::State { keys, .. } => assert!(keys.is_down(31)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn panic_key_and_timeout_return_to_local() {
        let (mut c, now) = controller(PointerMode::Absolute);
        enter(&mut c, now);
        assert_eq!(c.on_key(Some(70), true, true), Verdict::Block);
        assert!(!c.is_remote());

        enter(&mut c, now);
        c.tick(now + Duration::from_millis(CLIENT_TIMEOUT_MS + 1));
        assert!(!c.is_remote());
        assert_eq!(c.outbox, vec![Message::Leave]);
    }

    #[test]
    fn wheel_is_forwarded_only_in_remote_mode() {
        let (mut c, now) = controller(PointerMode::Absolute);
        let wheel = MouseEvent::Wheel {
            vertical: -120,
            horizontal: 0,
        };
        assert_eq!(c.on_mouse(wheel, now), Verdict::Pass);
        enter(&mut c, now);
        assert_eq!(c.on_mouse(wheel, now), Verdict::Block);
        assert_eq!(
            c.outbox,
            vec![Message::Wheel {
                vertical: -120,
                horizontal: 0
            }]
        );
    }

    #[test]
    fn button_releases_pass_through() {
        let (mut c, now) = controller(PointerMode::Absolute);
        enter(&mut c, now);
        let right = |down| MouseEvent::Button {
            button: Button::Right,
            down,
        };
        assert_eq!(c.on_mouse(right(false), now), Verdict::Pass);
        assert_eq!(c.on_mouse(right(true), now), Verdict::Block);
        assert_eq!(c.on_mouse(right(false), now), Verdict::Block);
    }
}
