//! Notification-area (tray) icon: shows whether bamf is active, paused or
//! waiting for the client, and offers Pause/Resume and Quit.
//!
//! Everything here runs on the main thread, inside the same message loop
//! that services the low-level hooks. The server lock is never held while a
//! menu is open: the hooks keep firing during `TrackPopupMenu` and need it.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::ffi::*;
use super::{server, settings};
use crate::icon::{self, Look};

/// Sent by the shell when the icon is clicked.
const WM_TRAY: u32 = WM_APP + 1;
/// Posted by other threads when the status may have changed.
const WM_REFRESH: u32 = WM_APP + 2;

const ID_TOGGLE: usize = 1;
const ID_QUIT: usize = 2;
const ID_SETTINGS: usize = 3;
const ICON_ID: u32 = 1;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static LAST_STATUS: Mutex<Option<Status>> = Mutex::new(None);

/// Handlers for messages the tray window receives besides its own.
pub struct Hooks {
    pub clipboard_changed: fn(),
}

static HOOKS: Mutex<Option<Hooks>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Absent,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub paused: bool,
    pub client: Client,
    pub remote: bool,
}

impl Status {
    fn tooltip(&self) -> String {
        let state = if self.paused {
            "paused"
        } else if self.remote {
            "input on the client"
        } else {
            "active"
        };
        let client = match self.client {
            Client::Absent => "client not connected",
            Client::Connected => "client connected",
        };
        format!("bamf: {state}, {client}")
    }

    fn client_line(&self) -> &'static str {
        match self.client {
            Client::Absent => "Client: not connected",
            Client::Connected => "Client: connected",
        }
    }
}

pub fn current_status() -> Status {
    let s = server();
    let c = &s.controller;
    let client = if c.client_alive(Instant::now()) {
        Client::Connected
    } else {
        Client::Absent
    };
    Status {
        paused: c.is_paused(),
        client,
        remote: c.is_remote(),
    }
}

/// Asks the tray to redraw if `status` differs from what it shows. Safe to
/// call from any thread, without holding the server lock.
pub fn notify(status: Status) {
    let mut last = LAST_STATUS.lock().unwrap_or_else(|p| p.into_inner());
    if *last != Some(status) {
        *last = Some(status);
        let hwnd = WINDOW.load(Ordering::Relaxed);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd, WM_REFRESH, 0, 0);
            }
        }
    }
}

pub fn window() -> HWND {
    WINDOW.load(Ordering::Relaxed)
}

/// Creates the hidden window that owns the icon (and receives clipboard
/// notifications) and adds the icon.
pub fn create(hooks: Hooks) -> Result<HWND, String> {
    *HOOKS.lock().unwrap_or_else(|p| p.into_inner()) = Some(hooks);
    let class = wide("bamf-tray");
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: wndproc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: 0,
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("cannot register the tray window class".into());
        }
        // A regular top-level window that is never shown: message-only
        // windows miss broadcasts such as the shell restarting.
        let title = wide("bamf");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            instance,
            std::ptr::null_mut(),
        );
        if hwnd == 0 {
            return Err("cannot create the tray window".into());
        }
        WINDOW.store(hwnd, Ordering::Relaxed);

        let mut data = icon_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icons().active;
        set_tip(&mut data, "bamf");
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            bamf_proto::warn!("cannot add the tray icon; use Ctrl+C to quit");
        }
        Ok(hwnd)
    }
}

pub fn remove() {
    let hwnd = window();
    if hwnd != 0 {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
        }
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    // All-zero is a valid NOTIFYICONDATAW.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = ICON_ID;
    data
}

fn set_tip(data: &mut NOTIFYICONDATAW, text: &str) {
    for (dst, src) in data
        .szTip
        .iter_mut()
        .zip(text.encode_utf16().take(127).chain(std::iter::once(0)))
    {
        *dst = src;
    }
}

fn refresh(hwnd: HWND) {
    let status = current_status();
    *LAST_STATUS.lock().unwrap_or_else(|p| p.into_inner()) = Some(status);
    let icons = icons();
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_ICON | NIF_TIP;
    data.hIcon = if status.paused {
        icons.paused
    } else if status.client == Client::Absent {
        icons.waiting
    } else {
        icons.active
    };
    set_tip(&mut data, &status.tooltip());
    unsafe {
        Shell_NotifyIconW(NIM_MODIFY, &data);
    }
    settings::client_changed(status.client == Client::Connected);
}

fn toggle_pause() {
    let mut s = server();
    let paused = !s.controller.is_paused();
    s.controller.set_paused(paused);
    s.flush();
}

fn show_menu(hwnd: HWND) {
    let status = current_status();
    let toggle_label = if status.paused { "Resume" } else { "Pause" };
    unsafe {
        let menu = CreatePopupMenu();
        if menu == 0 {
            return;
        }
        let client = wide(status.client_line());
        let toggle = wide(toggle_label);
        let quit = wide("Quit");
        let settings_label = wide("Settings…");
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, client.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_SETTINGS, settings_label.as_ptr());
        let check = if status.paused { MF_CHECKED } else { 0 };
        AppendMenuW(menu, MF_STRING | check, ID_TOGGLE, toggle.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_QUIT, quit.as_ptr());

        let mut pt = POINT::default();
        GetCursorPos(&mut pt);
        // Without this the menu does not close when clicking elsewhere.
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as usize;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match cmd {
            ID_TOGGLE => {
                toggle_pause();
                refresh(hwnd);
            }
            ID_SETTINGS => settings::open(),
            ID_QUIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            // Left click pauses or resumes right away; right click opens the
            // menu (status and Quit).
            match lparam as u32 {
                WM_LBUTTONUP => {
                    toggle_pause();
                    refresh(hwnd);
                }
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_REFRESH => {
            refresh(hwnd);
            0
        }
        WM_CLIPBOARDUPDATE => {
            let hook = HOOKS
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .map(|h| h.clipboard_changed);
            if let Some(hook) = hook {
                hook();
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// The awake imp, for the settings window's title bar.
pub fn app_icon() -> HICON {
    icons().active
}

/// The awake imp at the large-icon size, for the taskbar and Alt+Tab.
pub fn app_icon_big() -> HICON {
    static BIG: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BIG.get_or_init(|| make_icon_sized(Look::Awake, metric(SM_CXICON, 32)) as usize) as HICON
}

fn metric(index: i32, fallback: usize) -> usize {
    match unsafe { GetSystemMetrics(index) } {
        n if n > 0 => n as usize,
        _ => fallback,
    }
}

struct Icons {
    active: HICON,
    waiting: HICON,
    paused: HICON,
}

fn icons() -> &'static Icons {
    static ICONS: std::sync::OnceLock<Icons> = std::sync::OnceLock::new();
    ICONS.get_or_init(|| Icons {
        active: make_icon(Look::Awake),
        waiting: make_icon(Look::Dozing),
        paused: make_icon(Look::Asleep),
    })
}

/// Draws the pixel-art imp at the shell's small-icon size (16 px at 100%
/// scaling), so the shell does not have to resample it.
fn make_icon(look: Look) -> HICON {
    make_icon_sized(look, metric(SM_CXSMICON, 16))
}

fn make_icon_sized(look: Look, size: usize) -> HICON {
    // The art is fully opaque or fully transparent, so straight and
    // premultiplied alpha are the same; only RGBA -> BGRA is needed.
    let mut bgra = icon::render(look, size);
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    // AND mask rows are padded to 16 bits; all zero lets the alpha decide.
    let and = vec![0u8; size.div_ceil(16) * 2 * size];
    unsafe {
        CreateIcon(
            0,
            size as i32,
            size as i32,
            1,
            32,
            and.as_ptr(),
            bgra.as_ptr(),
        )
    }
}
