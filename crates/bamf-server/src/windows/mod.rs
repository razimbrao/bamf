//! Windows glue: low-level hooks, cursor handling and networking threads
//! around the platform-independent [`Controller`].

pub mod app;
mod clipboard;
mod ffi;
mod sync;
mod tray;

use std::net::{SocketAddr, UdpSocket};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use bamf_proto::{Button, Direction, Key, Message, Session, STATE_INTERVAL_MS};

use crate::controller::{Controller, MouseEvent, Platform, Verdict};
use crate::geometry::Rect;
use crate::keymap;
use crate::Settings;
use ffi::*;

struct WinPlatform;

impl Platform for WinPlatform {
    fn hide_cursor(&mut self) {
        // A 32x32 cursor whose AND mask keeps the screen and XOR mask adds
        // nothing: fully transparent. SetSystemCursor takes ownership of the
        // handle, so a new one is created for every system cursor.
        let and_mask = [0xffu8; 128];
        let xor_mask = [0u8; 128];
        for id in SYSTEM_CURSORS {
            unsafe {
                let blank = CreateCursor(0, 0, 0, 32, 32, and_mask.as_ptr(), xor_mask.as_ptr());
                if blank != 0 {
                    SetSystemCursor(blank, id);
                }
            }
        }
    }

    fn show_cursor(&mut self) {
        restore_cursors();
    }

    fn warp_cursor(&mut self, x: i32, y: i32) {
        unsafe {
            SetCursorPos(x, y);
        }
    }
}

fn restore_cursors() {
    unsafe {
        SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), 0);
    }
}

struct Server {
    controller: Controller<WinPlatform>,
    session: Session,
    socket: UdpSocket,
    client: Option<SocketAddr>,
}

impl Server {
    /// Seals and sends everything the controller queued.
    fn flush(&mut self) {
        let Some(addr) = self.client else {
            self.controller.outbox.clear();
            return;
        };
        for msg in self.controller.outbox.drain(..) {
            let datagram = self.session.seal(&msg);
            if let Err(e) = self.socket.send_to(&datagram, addr) {
                bamf_proto::debug!("send to {addr} failed: {e}");
            }
        }
    }
}

static SERVER: OnceLock<Mutex<Server>> = OnceLock::new();

fn server() -> MutexGuard<'static, Server> {
    // A panic while holding the lock must not take input down with it.
    SERVER
        .get()
        .expect("server not initialised")
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.flags & LLMHF_INJECTED == 0 {
            if let Some(event) = translate_mouse(wparam as u32, info) {
                let mut s = server();
                let verdict = s.controller.on_mouse(event, Instant::now());
                s.flush();
                if verdict == Verdict::Block {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(0, code, wparam, lparam)
}

fn translate_mouse(message: u32, info: &MSLLHOOKSTRUCT) -> Option<MouseEvent> {
    let high = (info.mouseData >> 16) as u16;
    let button = |button, down| Some(MouseEvent::Button { button, down });
    match message {
        WM_MOUSEMOVE => Some(MouseEvent::Move {
            x: info.pt.x,
            y: info.pt.y,
        }),
        WM_LBUTTONDOWN => button(Button::Left, true),
        WM_LBUTTONUP => button(Button::Left, false),
        WM_RBUTTONDOWN => button(Button::Right, true),
        WM_RBUTTONUP => button(Button::Right, false),
        WM_MBUTTONDOWN => button(Button::Middle, true),
        WM_MBUTTONUP => button(Button::Middle, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let b = if high == XBUTTON1 {
                Button::Back
            } else {
                Button::Forward
            };
            button(b, message == WM_XBUTTONDOWN)
        }
        WM_MOUSEWHEEL => Some(MouseEvent::Wheel {
            vertical: high as i16,
            horizontal: 0,
        }),
        WM_MOUSEHWHEEL => Some(MouseEvent::Wheel {
            vertical: 0,
            horizontal: high as i16,
        }),
        _ => None,
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        if info.flags & LLKHF_INJECTED == 0 {
            let message = wparam as u32;
            let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let extended = info.flags & LLKHF_EXTENDED != 0;
            let evdev = keymap::to_evdev(info.scanCode, extended, info.vkCode);
            let panic_key = info.vkCode == VK_SCROLL;

            let mut s = server();
            let verdict = s.controller.on_key(evdev, down, panic_key);
            s.flush();
            if verdict == Verdict::Block {
                return 1;
            }
        }
    }
    CallNextHookEx(0, code, wparam, lparam)
}

unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> BOOL {
    // Ctrl+C / console closed: give the cursor back before dying.
    if let Some(lock) = SERVER.get() {
        if let Ok(mut s) = lock.try_lock() {
            s.controller.force_local();
            s.flush();
        }
    }
    tray::remove();
    restore_cursors();
    0 // let the default handler terminate the process
}

fn desktop_rect() -> Rect {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        Rect {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

fn receive_loop(socket: UdpSocket) {
    let mut buf = [0u8; 1500];
    loop {
        let (len, addr) = match socket.recv_from(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                // Windows reports ICMP "port unreachable" from earlier sends
                // as an error on recv; it is harmless.
                bamf_proto::debug!("recv error: {e}");
                continue;
            }
        };
        let mut s = server();
        match s.session.open(&buf[..len]) {
            Ok(Message::Ping { pause }) => {
                if s.client != Some(addr) {
                    bamf_proto::info!("client connected from {addr}");
                    s.client = Some(addr);
                }
                let reply = s.controller.on_ping(Instant::now(), pause);
                s.controller.outbox.push(reply);
                s.flush();
                drop(s);
                tray::notify(tray::current_status());
            }
            Ok(other) => bamf_proto::debug!("unexpected message from client: {other:?}"),
            Err(e) => bamf_proto::debug!("dropped datagram from {addr}: {e}"),
        }
    }
}

fn heartbeat_loop() {
    loop {
        std::thread::sleep(Duration::from_millis(STATE_INTERVAL_MS));
        {
            let mut s = server();
            s.controller.tick(Instant::now());
            s.flush();
        }
        tray::notify(tray::current_status());
    }
}

pub fn random_key() -> Key {
    let mut bytes = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            0,
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    assert_eq!(status, 0, "BCryptGenRandom failed with status {status:#x}");
    Key::from_bytes(bytes)
}

pub fn run(settings: Settings) -> Result<(), String> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // In case a previous run crashed while the cursor was hidden.
    restore_cursors();

    let socket = UdpSocket::bind(&settings.listen)
        .map_err(|e| format!("cannot listen on {}: {e}", settings.listen))?;
    let send_socket = socket
        .try_clone()
        .map_err(|e| format!("cannot clone socket: {e}"))?;

    let local = desktop_rect();
    let park = unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN) / 2,
            GetSystemMetrics(SM_CYSCREEN) / 2,
        )
    };
    let geometry = crate::geometry::Geometry {
        local,
        remote_width: settings.remote_width,
        remote_height: settings.remote_height,
        side: settings.side,
        offset: settings.offset,
    };
    bamf_proto::info!(
        "desktop {}x{} at ({}, {}); remote {}x{} on the {:?} ({}); pointer {:?}",
        local.width(),
        local.height(),
        local.left,
        local.top,
        settings.remote_width,
        settings.remote_height,
        settings.side,
        match settings.offset {
            Some(o) => format!("offset {o} px"),
            None => "stretched".to_string(),
        },
        settings.mode
    );

    let controller = Controller::new(WinPlatform, geometry, settings.mode, park);
    SERVER
        .set(Mutex::new(Server {
            controller,
            session: Session::new(&settings.key, Direction::ServerToClient),
            socket: send_socket,
            client: None,
        }))
        .map_err(|_| "server already running".to_string())?;

    std::thread::spawn(move || receive_loop(socket));
    std::thread::spawn(heartbeat_loop);

    unsafe {
        SetConsoleCtrlHandler(Some(ctrl_handler), 1);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, mouse_hook, 0, 0);
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, keyboard_hook, 0, 0);
        if mouse == 0 || keyboard == 0 {
            return Err("failed to install input hooks".into());
        }
        let hwnd = tray::create(tray::Hooks {
            clipboard_changed: sync::clipboard_changed,
        })?;
        sync::start(&settings.listen, &settings.key)?;
        if AddClipboardFormatListener(hwnd) == 0 {
            bamf_proto::warn!("cannot watch the clipboard; it will not be synced");
        }
        bamf_proto::info!(
            "listening on {} — waiting for the client (Scroll Lock = back to Windows)",
            settings.listen
        );

        // Low-level hooks are called on this thread while it pumps messages;
        // the tray window's messages are dispatched by the same loop.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(mouse);
        UnhookWindowsHookEx(keyboard);
    }
    tray::remove();
    {
        let mut s = server();
        s.controller.force_local();
        s.flush();
    }
    restore_cursors();
    bamf_proto::info!("bye");
    Ok(())
}
