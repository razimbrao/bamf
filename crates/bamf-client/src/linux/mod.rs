//! Linux glue: UDP loop plus injection through uinput virtual devices.

mod control;
mod sync;
mod uinput;

pub use control::send_command;

use std::io::{ErrorKind, Read};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use bamf_proto::{
    Button, Direction, Key, Message, Session, ABS_MAX, CLIENT_TIMEOUT_MS, PING_INTERVAL_MS,
    SERVER_TIMEOUT_MS,
};

use bamf_proto::stream::{EchoGuard, MAX_CLIP};

use crate::control::{clip_line, Command, Status};
use crate::state::{Action, ClientState};
use crate::Settings;
use uinput::*;

const WHEEL_NOTCH: i32 = 120;

/// The three virtual devices and the glue that routes actions to them.
struct Injector {
    keyboard: VirtualDevice,
    /// Relative mouse, used for `MouseRel`.
    mouse: VirtualDevice,
    /// Absolute pointer (like a VM tablet), used for `MouseAbs`.
    pointer: VirtualDevice,
    absolute: bool,
    /// Wheel remainder in 1/120 notch units, for the low-resolution axes.
    wheel_rest: (i32, i32),
}

fn button_code(button: Button) -> u16 {
    match button {
        Button::Left => BTN_LEFT,
        Button::Right => BTN_RIGHT,
        Button::Middle => BTN_MIDDLE,
        Button::Back => BTN_SIDE,
        Button::Forward => BTN_EXTRA,
    }
}

impl Injector {
    fn new() -> std::io::Result<Self> {
        let wheels = vec![REL_WHEEL, REL_HWHEEL, REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES];
        let keyboard = VirtualDevice::create(
            "bamf keyboard",
            1,
            &Capabilities {
                keys: (1..=KEY_LAST_KEYBOARD).collect(),
                ..Default::default()
            },
        )?;
        let mut rel = vec![REL_X, REL_Y];
        rel.extend(&wheels);
        let mouse = VirtualDevice::create(
            "bamf mouse",
            2,
            &Capabilities {
                keys: MOUSE_BUTTONS.to_vec(),
                rel,
                abs: vec![],
            },
        )?;
        let pointer = VirtualDevice::create(
            "bamf pointer",
            3,
            &Capabilities {
                keys: MOUSE_BUTTONS.to_vec(),
                rel: wheels,
                abs: vec![(ABS_X, ABS_MAX as i32), (ABS_Y, ABS_MAX as i32)],
            },
        )?;
        Ok(Self {
            keyboard,
            mouse,
            pointer,
            absolute: true,
            wheel_rest: (0, 0),
        })
    }

    fn pointing(&mut self) -> &mut VirtualDevice {
        if self.absolute {
            &mut self.pointer
        } else {
            &mut self.mouse
        }
    }

    fn wheel(&mut self, vertical: i16, horizontal: i16) {
        self.wheel_rest.0 += vertical as i32;
        self.wheel_rest.1 += horizontal as i32;
        let notches = (
            self.wheel_rest.0 / WHEEL_NOTCH,
            self.wheel_rest.1 / WHEEL_NOTCH,
        );
        self.wheel_rest.0 %= WHEEL_NOTCH;
        self.wheel_rest.1 %= WHEEL_NOTCH;

        let dev = self.pointing();
        if vertical != 0 {
            dev.emit(EV_REL, REL_WHEEL_HI_RES, vertical as i32);
        }
        if horizontal != 0 {
            dev.emit(EV_REL, REL_HWHEEL_HI_RES, horizontal as i32);
        }
        if notches.0 != 0 {
            dev.emit(EV_REL, REL_WHEEL, notches.0);
        }
        if notches.1 != 0 {
            dev.emit(EV_REL, REL_HWHEEL, notches.1);
        }
    }

    fn apply(&mut self, actions: &[Action]) -> std::io::Result<()> {
        for action in actions {
            match *action {
                Action::Key { code, down } => self.keyboard.emit(EV_KEY, code, down as i32),
                Action::Button { button, down } => {
                    self.pointing()
                        .emit(EV_KEY, button_code(button), down as i32)
                }
                Action::MoveAbs { x, y } => {
                    self.absolute = true;
                    self.pointer.emit(EV_ABS, ABS_X, x as i32);
                    self.pointer.emit(EV_ABS, ABS_Y, y as i32);
                }
                Action::MoveRel { dx, dy } => {
                    self.absolute = false;
                    self.mouse.emit(EV_REL, REL_X, dx as i32);
                    self.mouse.emit(EV_REL, REL_Y, dy as i32);
                }
                Action::Wheel {
                    vertical,
                    horizontal,
                } => self.wheel(vertical, horizontal),
            }
        }
        self.keyboard.sync()?;
        self.mouse.sync()?;
        self.pointer.sync()
    }
}

pub fn random_key() -> Result<Key, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("cannot read /dev/urandom: {e}"))?;
    Ok(Key::from_bytes(bytes))
}

pub fn run(settings: Settings) -> Result<(), String> {
    let mut injector = Injector::new().map_err(|e| {
        format!(
            "cannot create virtual input devices ({e}). \
             Is the udev rule for /dev/uinput installed? See README."
        )
    })?;

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("cannot bind socket: {e}"))?;
    socket
        .connect(&settings.server)
        .map_err(|e| format!("cannot resolve {}: {e}", settings.server))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;

    let mut control = control::Control::bind()?;
    let clipboard = sync::start(settings.server.clone(), settings.key.clone());
    let mut echo = EchoGuard::default();
    let mut session = Session::new(&settings.key, Direction::ClientToServer);
    let mut state = ClientState::default();
    let mut actions = Vec::new();
    let mut buf = [0u8; 1500];
    let mut last_ping: Option<Instant> = None;
    let mut last_rx: Option<Instant> = None;
    let ping_every = Duration::from_millis(PING_INTERVAL_MS);
    let server_timeout = Duration::from_millis(SERVER_TIMEOUT_MS);
    let link_timeout = Duration::from_millis(CLIENT_TIMEOUT_MS);
    let mut paused = false;
    let mut shown = Status::default();

    bamf_proto::info!(
        "pinging server at {}; control socket {}",
        settings.server,
        control::socket_path().display()
    );

    loop {
        let status = Status {
            connected: last_rx.is_some_and(|t| t.elapsed() < link_timeout),
            remote: state.is_active(),
            paused,
            server_paused: state.is_server_paused(),
        };
        let mut force_status = false;
        for cmd in control.poll(&status.line()) {
            let was = paused;
            match cmd {
                Command::Pause => paused = true,
                Command::Resume => paused = false,
                Command::Toggle => paused = !paused,
                Command::Status => {}
                Command::Clip { mime, data } => {
                    if data.len() > MAX_CLIP {
                        bamf_proto::warn!(
                            "clipboard content too large to sync ({} bytes)",
                            data.len()
                        );
                    } else if echo.should_send(&mime, &data) {
                        let _ = clipboard.outgoing.send((mime, data));
                    }
                    // Not a state change: no status line.
                    continue;
                }
            }
            if paused != was {
                bamf_proto::info!("{}", if paused { "paused" } else { "resumed" });
                // Tell the server right away instead of at the next ping.
                last_ping = None;
            }
            force_status = true;
        }
        while let Ok((mime, data)) = clipboard.incoming.try_recv() {
            echo.received(&mime, &data);
            control.broadcast(&clip_line(&mime, &data));
        }
        let status = Status { paused, ..status };
        if status != shown || force_status {
            control.broadcast(&status.line());
            shown = status;
        }

        if last_ping.is_none_or(|t| t.elapsed() >= ping_every) {
            // Errors (e.g. server not up yet) are expected; keep trying.
            let _ = socket.send(&session.seal(&Message::Ping { paused }));
            last_ping = Some(Instant::now());
        }

        match socket.recv(&mut buf) {
            Ok(len) => match session.open(&buf[..len]) {
                Ok(msg) => {
                    if last_rx.is_none() {
                        bamf_proto::info!("server reachable");
                    }
                    last_rx = Some(Instant::now());
                    let was_active = state.is_active();
                    state.handle(msg, &mut actions);
                    if state.is_active() != was_active {
                        bamf_proto::info!(
                            "{}",
                            if state.is_active() {
                                "bamf! -> here"
                            } else {
                                "bamf! <- back to server"
                            }
                        );
                    }
                }
                Err(e) => bamf_proto::debug!("dropped datagram: {e}"),
            },
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionRefused
                ) => {}
            Err(e) => bamf_proto::warn!("recv error: {e}"),
        }

        if state.is_active() && last_rx.is_some_and(|t| t.elapsed() > server_timeout) {
            bamf_proto::warn!("server went silent, releasing all keys");
            state.timeout(&mut actions);
        }

        if !actions.is_empty() {
            if let Err(e) = injector.apply(&actions) {
                bamf_proto::warn!("injection failed: {e}");
            }
            actions.clear();
        }
    }
}
