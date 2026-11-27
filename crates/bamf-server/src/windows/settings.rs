//! The settings window, opened from the tray menu: arranges the screens and
//! shows the clipboard history. Content and input logic live in
//! [`crate::panel`]; this module only creates the window, paints the
//! panel's pixels and text, and carries out its actions.
//!
//! Everything runs on the main thread, next to the input hooks, so painting
//! must stay quick: the panel draws into a pixel buffer that is copied to
//! the window in one call.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use bamf_proto::{MIME_PNG, MIME_TEXT};

use super::ffi::*;
use super::{clipboard, desktop_rect, server, sync, tray};
use crate::canvas::{Area, Canvas};
use crate::geometry::Rect;
use crate::history::{thumbnail, Content, History, Origin};
use crate::image::{self, Image};
use crate::layout::{update_config, Arrangement, Screens};
use crate::panel::{self, Action, Align, Key, Monitor, Panel, HEIGHT, WIDTH};

/// Posted when the history changed on another thread.
const WM_HISTORY: u32 = WM_APP + 10;
/// Ages in the list are refreshed on this timer.
const TIMER_AGES: usize = 1;
/// Closes the monitor numbers shown by "identify".
const TIMER_IDENTIFY: usize = 2;

const STYLE: u32 = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

static HISTORY: Mutex<History> = Mutex::new(History::new());
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static CONTEXT: OnceLock<Context> = OnceLock::new();

/// Fixed facts from startup.
struct Context {
    config_path: PathBuf,
    pointer_mode: String,
}

struct State {
    panel: Panel,
    canvas: Canvas,
    fonts: HashMap<i32, HGDIOBJ>,
    identify: Vec<HWND>,
}

thread_local! {
    // Main thread only. Never hold the borrow across calls that may send
    // messages back to the window procedure.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn history() -> MutexGuard<'static, History> {
    HISTORY.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn init(config_path: PathBuf, pointer_mode: &str) {
    let _ = CONTEXT.set(Context {
        config_path,
        pointer_mode: pointer_mode.to_string(),
    });
}

/// Adds clipboard content to the history. Called from the sync threads;
/// `decoded` saves decoding a PNG that was just encoded.
pub fn record(mime: &str, data: &[u8], decoded: Option<&Image>, origin: Origin) {
    let content = if mime.starts_with("text/") {
        Content::Text(String::from_utf8_lossy(data).into_owned())
    } else if mime == MIME_PNG {
        let owned;
        let img = match decoded {
            Some(img) => img,
            None => match image::png::decode(data) {
                Ok(img) => {
                    owned = img;
                    &owned
                }
                Err(e) => {
                    bamf_proto::debug!("history: cannot decode the image: {e}");
                    return;
                }
            },
        };
        let (w, h) = panel::THUMB_SIZE;
        Content::Image {
            png: data.to_vec(),
            width: img.width,
            height: img.height,
            thumb: thumbnail(img, w, h, panel::LIST_BG),
        }
    } else {
        return;
    };
    if history().push(content, origin, Instant::now()) {
        let hwnd = WINDOW.load(Ordering::Relaxed);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd, WM_HISTORY, 0, 0);
            }
        }
    }
}

/// Called by the tray when the client connects or goes away.
pub fn client_changed(online: bool) {
    let hwnd = WINDOW.load(Ordering::Relaxed);
    if hwnd == 0 {
        return;
    }
    let changed = with_state(|s| {
        let changed = s.panel.client_online != online;
        s.panel.client_online = online;
        changed
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

fn invalidate(hwnd: HWND) {
    unsafe {
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

/// Scale for the pixel art: whole numbers only, so pixels stay square.
fn scale_for(dpi: u32) -> i32 {
    ((dpi as i32 + 48) / 96).max(1)
}

/// Opens the window, or brings it to the front if it is already open.
pub fn open() {
    let existing = WINDOW.load(Ordering::Relaxed);
    if existing != 0 {
        unsafe {
            if IsIconic(existing) != 0 {
                ShowWindow(existing, SW_RESTORE);
            }
            SetForegroundWindow(existing);
        }
        return;
    }
    if let Err(e) = create() {
        bamf_proto::warn!("cannot open the settings window: {e}");
    }
}

fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data as *mut Vec<Monitor>);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(m, &mut info) != 0 {
            let r = info.rcMonitor;
            list.push(Monitor {
                rect: Rect {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        1
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(0, std::ptr::null(), each, &mut list as *mut _ as LPARAM);
    }
    list.sort_by_key(|m| (m.rect.left, m.rect.top));
    list
}

fn work_area_of_primary() -> RECT {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(m, &mut info) != 0 && info.dwFlags & MONITORINFOF_PRIMARY != 0 {
            *(data as *mut RECT) = info.rcWork;
            return 0;
        }
        1
    }
    let mut work = RECT {
        left: 0,
        top: 0,
        right: 1280,
        bottom: 720,
    };
    unsafe {
        EnumDisplayMonitors(0, std::ptr::null(), each, &mut work as *mut _ as LPARAM);
    }
    work
}

fn new_panel() -> Panel {
    let ctx = CONTEXT.get();
    let (geometry, online) = {
        let s = server();
        (
            s.controller.geometry(),
            s.controller.client_alive(Instant::now()),
        )
    };
    let screens = Screens {
        desktop: desktop_rect(),
        remote_width: geometry.remote_width as i32,
        remote_height: geometry.remote_height as i32,
    };
    let saved = Arrangement {
        side: geometry.side,
        offset: geometry.offset,
    };
    let mode = ctx.map_or("absolute", |c| c.pointer_mode.as_str());
    let mut panel = Panel::new(screens, monitors(), saved, mode);
    panel.client_online = online;
    panel
}

fn register_class(name: &[u16], proc_: WndProc) {
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: proc_,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: name.as_ptr(),
            hIconSm: 0,
        };
        // Fails harmlessly when already registered (window reopened).
        RegisterClassExW(&wc);
    }
}

fn create() -> Result<(), String> {
    let class = wide("bamf-settings");
    register_class(&class, wndproc);
    let title = wide("BAMF");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            STYLE,
            0,
            0,
            100,
            100,
            0,
            0,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null_mut(),
        )
    };
    if hwnd == 0 {
        return Err("CreateWindowExW failed".into());
    }
    WINDOW.store(hwnd, Ordering::Relaxed);

    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let scale = scale_for(dpi);
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            panel: new_panel(),
            canvas: Canvas::new(WIDTH, HEIGHT, scale),
            fonts: HashMap::new(),
            identify: Vec::new(),
        })
    });

    unsafe {
        let on: BOOL = 1;
        let dark = |attr: u32, v: *const std::ffi::c_void, n: usize| {
            DwmSetWindowAttribute(hwnd, attr, v, n as u32);
        };
        dark(DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let caption = bgr(panel::BG);
        let border = bgr(0x0b0e1e);
        let text = bgr(0xe6e3f5);
        dark(DWMWA_CAPTION_COLOR, &caption as *const _ as _, 4);
        dark(DWMWA_BORDER_COLOR, &border as *const _ as _, 4);
        dark(DWMWA_TEXT_COLOR, &text as *const _ as _, 4);

        SendMessageW(hwnd, WM_SETICON, ICON_SMALL, tray::app_icon());
        SendMessageW(hwnd, WM_SETICON, ICON_BIG, tray::app_icon_big());

        // Centre on the primary monitor's work area.
        let (w, h) = outer_size(scale, dpi);
        let work = work_area_of_primary();
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;
        SetWindowPos(hwnd, 0, x, y, w, h, SWP_NOZORDER);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetTimer(hwnd, TIMER_AGES, 30_000, std::ptr::null());
    }
    Ok(())
}

/// Window size for a client area of the panel at `scale`.
fn outer_size(scale: i32, dpi: u32) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: WIDTH * scale,
        bottom: HEIGHT * scale,
    };
    unsafe {
        AdjustWindowRectExForDpi(&mut r, STYLE, 0, 0, dpi);
    }
    (r.right - r.left, r.bottom - r.top)
}

/// 0xRRGGBB → COLORREF (0x00BBGGRR).
fn bgr(c: u32) -> u32 {
    (c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff
}

fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    with_state(|s| {
        let runs = {
            let h = history();
            s.panel.render(&mut s.canvas, &h, Instant::now())
        };
        let c = &s.canvas;
        let scale = c.scale;
        unsafe {
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, c.width, c.height);
            let old_bmp = SelectObject(mem, bmp);
            let info = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: c.width,
                biHeight: -c.height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            SetDIBitsToDevice(
                mem,
                0,
                0,
                c.width as u32,
                c.height as u32,
                0,
                0,
                0,
                c.height as u32,
                c.pixels.as_ptr() as _,
                &info,
                DIB_RGB_COLORS,
            );
            SetBkMode(mem, TRANSPARENT);
            let old_font = SelectObject(mem, font(&mut s.fonts, 12 * scale));
            for run in &runs {
                SelectObject(mem, font(&mut s.fonts, run.size * scale));
                SetTextColor(mem, bgr(run.color));
                let a = run.area;
                let mut r = RECT {
                    left: a.x * scale,
                    top: a.y * scale,
                    right: a.right() * scale,
                    bottom: a.bottom() * scale,
                };
                let align = match run.align {
                    Align::Left => 0,
                    Align::Centre => DT_CENTER,
                    Align::Right => DT_RIGHT,
                };
                let text: Vec<u16> = run.text.encode_utf16().collect();
                DrawTextW(
                    mem,
                    text.as_ptr(),
                    text.len() as i32,
                    &mut r,
                    DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | align,
                );
            }
            SelectObject(mem, old_font);
            BitBlt(hdc, 0, 0, c.width, c.height, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp);
            DeleteDC(mem);
        }
    });
    unsafe {
        EndPaint(hwnd, &ps);
    }
}

fn font(cache: &mut HashMap<i32, HGDIOBJ>, px: i32) -> HGDIOBJ {
    *cache.entry(px).or_insert_with(|| {
        let face = wide("Consolas");
        unsafe {
            CreateFontW(
                -px,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                0,
                0,
                CLEARTYPE_QUALITY,
                0,
                face.as_ptr(),
            )
        }
    })
}

/// Mouse position in logical pixels.
fn mouse_at(lparam: LPARAM, scale: i32) -> (i32, i32) {
    let x = (lparam & 0xffff) as u16 as i16 as i32;
    let y = ((lparam >> 16) & 0xffff) as u16 as i16 as i32;
    (x.div_euclid(scale), y.div_euclid(scale))
}

fn key_of(vk: i32) -> Option<Key> {
    Some(match vk {
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_RETURN => Key::Enter,
        VK_DELETE => Key::Delete,
        VK_BACK => Key::Backspace,
        VK_ESCAPE => Key::Escape,
        VK_TAB => Key::Tab,
        _ => return None,
    })
}

fn pressed(vk: i32) -> bool {
    unsafe { GetKeyState(vk) < 0 }
}

/// Carries out what the panel asked for, outside the state borrow.
fn perform(hwnd: HWND, action: Action) {
    match action {
        Action::None => return,
        Action::Redraw => {}
        Action::Apply(a) => {
            let result = apply(a);
            with_state(|s| {
                s.panel.status = Some(match &result {
                    Ok(msg) => (msg.clone(), false),
                    Err(e) => (e.clone(), true),
                });
                // Input already follows the new arrangement, even if the
                // file could not be written.
                s.panel.saved = a;
            });
        }
        Action::Identify => identify(hwnd),
        Action::Copy(id) => {
            let entry = history().get(id).map(|e| e.content.clone());
            if let Some(content) = entry {
                copy(content);
                with_state(|s| s.panel.status = Some(("copied to the clipboard".into(), false)));
            }
        }
        Action::TogglePin(id) => history().toggle_pin(id),
        Action::Delete(id) => history().remove(id),
        Action::Clear => history().clear(),
        Action::Close => {
            unsafe {
                DestroyWindow(hwnd);
            }
            return;
        }
    }
    let h = history();
    with_state(|s| s.panel.history_changed(&h));
    drop(h);
    invalidate(hwnd);
}

/// Uses `a` right away and saves it to the config file.
fn apply(a: Arrangement) -> Result<String, String> {
    {
        let mut s = server();
        let mut g = s.controller.geometry();
        g.side = a.side;
        g.offset = a.offset;
        s.controller.set_geometry(g);
        s.flush();
    }
    bamf_proto::info!(
        "screens: client on the {} ({})",
        a.side.name(),
        match a.offset {
            Some(o) => format!("offset {o} px"),
            None => "stretched".into(),
        }
    );
    let path = CONTEXT
        .get()
        .map(|c| c.config_path.clone())
        .ok_or("no config file")?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("applied, but cannot read {name}: {e}"))?;
    std::fs::write(&path, update_config(&text, a))
        .map_err(|e| format!("applied, but cannot save {name}: {e}"))?;
    Ok(format!("applied and saved to {name}"))
}

/// Puts a history entry back on the clipboard, which also syncs it to the
/// client. On its own thread: PNG → DIB can take a moment.
fn copy(content: Content) {
    sync::forget_echo();
    std::thread::spawn(move || {
        let (mime, data) = match content {
            Content::Text(t) => (MIME_TEXT, t.into_bytes()),
            Content::Image { png, .. } => (MIME_PNG, png),
        };
        if let Err(e) = clipboard::write(tray::window(), mime, &data) {
            bamf_proto::warn!("cannot set the clipboard: {e}");
        }
    });
}

// ---- identify: a big number on each local monitor for two seconds ----

fn identify(owner: HWND) {
    close_identify(owner);
    let class = wide("bamf-identify");
    register_class(&class, identify_proc);
    let mut shown = Vec::new();
    for (i, m) in monitors().iter().enumerate() {
        let size = 160;
        let (x, y) = (m.rect.left + 48, m.rect.bottom - size - 96);
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                x,
                y,
                size,
                size,
                owner,
                0,
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null_mut(),
            );
            if hwnd != 0 {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, i as isize + 1);
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                shown.push(hwnd);
            }
        }
    }
    with_state(|s| s.identify = shown);
    unsafe {
        SetTimer(owner, TIMER_IDENTIFY, 2000, std::ptr::null());
    }
}

fn close_identify(owner: HWND) {
    let list = with_state(|s| std::mem::take(&mut s.identify)).unwrap_or_default();
    unsafe {
        KillTimer(owner, TIMER_IDENTIFY);
        for hwnd in list {
            DestroyWindow(hwnd);
        }
    }
}

unsafe extern "system" fn identify_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let n = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            let mut c = Canvas::new(80, 80, 2);
            c.fill(Area::new(0, 0, 80, 80), panel::BG);
            c.frame(Area::new(0, 0, 80, 80), 3, 0x8a74e0);
            let label = n.to_string();
            let dot = 8;
            let w = crate::canvas::text_width(&label, dot);
            c.text((80 - w) / 2, (80 - 7 * dot) / 2, &label, 0xffe23a, dot);
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let info = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: c.width,
                biHeight: -c.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            SetDIBitsToDevice(
                hdc,
                0,
                0,
                c.width as u32,
                c.height as u32,
                0,
                0,
                0,
                c.height as u32,
                c.pixels.as_ptr() as _,
                &info,
                DIB_RGB_COLORS,
            );
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---- window procedure ----

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let scale = with_state(|s| s.canvas.scale).unwrap_or(1);
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let (x, y) = mouse_at(lparam, scale);
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tme);
            let redraw = {
                let h = history();
                with_state(|s| s.panel.mouse_move(x, y, &h)).unwrap_or(false)
            };
            if redraw {
                invalidate(hwnd);
            }
            0
        }
        WM_MOUSELEAVE => {
            if with_state(|s| !s.panel.dragging() && s.panel.mouse_leave()) == Some(true) {
                invalidate(hwnd);
            }
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = mouse_at(lparam, scale);
            SetCapture(hwnd);
            let action = {
                let h = history();
                with_state(|s| s.panel.mouse_down(x, y, &h))
            };
            if let Some(a) = action {
                perform(hwnd, a);
            }
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_at(lparam, scale);
            ReleaseCapture();
            let action = {
                let h = history();
                with_state(|s| s.panel.mouse_up(x, y, &h))
            };
            if let Some(a) = action {
                perform(hwnd, a);
            }
            0
        }
        WM_SETCURSOR => {
            let hand =
                with_state(|s| s.panel.dragging() || s.panel.over_control()).unwrap_or(false);
            if hand && (lparam & 0xffff) as u32 == 1 {
                // HTCLIENT
                let dragging = with_state(|s| s.panel.dragging()).unwrap_or(false);
                SetCursor(LoadCursorW(
                    0,
                    if dragging { IDC_SIZEALL } else { IDC_HAND },
                ));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEWHEEL => {
            let notches = ((wparam >> 16) as u16 as i16) as i32 / 120;
            let redraw = {
                let h = history();
                with_state(|s| s.panel.wheel(notches, &h)).unwrap_or(false)
            };
            if redraw {
                invalidate(hwnd);
            }
            0
        }
        WM_KEYDOWN => {
            if let Some(key) = key_of(wparam as i32) {
                let shift = pressed(VK_SHIFT);
                let action = {
                    let h = history();
                    with_state(|s| s.panel.key(key, shift, &h))
                };
                if let Some(a) = action {
                    perform(hwnd, a);
                }
                return 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CHAR => {
            let c = char::from_u32(wparam as u32).unwrap_or('\0');
            if !pressed(VK_CONTROL) && with_state(|s| s.panel.char(c)) == Some(true) {
                invalidate(hwnd);
            }
            0
        }
        WM_TIMER => {
            match wparam {
                TIMER_IDENTIFY => close_identify(hwnd),
                _ => invalidate(hwnd),
            }
            0
        }
        WM_HISTORY => {
            let h = history();
            with_state(|s| s.panel.history_changed(&h));
            drop(h);
            invalidate(hwnd);
            0
        }
        WM_DPICHANGED => {
            let dpi = (wparam & 0xffff) as u32;
            let scale = scale_for(dpi);
            with_state(|s| {
                if s.canvas.scale != scale {
                    s.canvas = Canvas::new(WIDTH, HEIGHT, scale);
                }
            });
            let suggested = &*(lparam as *const RECT);
            let (w, h) = outer_size(scale, dpi);
            SetWindowPos(
                hwnd,
                0,
                suggested.left,
                suggested.top,
                w,
                h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            invalidate(hwnd);
            0
        }
        WM_DESTROY => {
            close_identify(hwnd);
            KillTimer(hwnd, TIMER_AGES);
            WINDOW.store(0, Ordering::Relaxed);
            let state = STATE.with(|cell| cell.borrow_mut().take());
            if let Some(state) = state {
                for (_, f) in state.fonts {
                    DeleteObject(f);
                }
            }
            // Not PostQuitMessage: closing the window must not stop bamf.
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
