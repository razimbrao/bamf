//! Hand-written bindings for the few Win32 APIs bamf needs (x86_64).

#![allow(non_snake_case, clippy::upper_case_acronyms, dead_code)]

use std::ffi::c_void;

pub type HHOOK = isize;
pub type HCURSOR = isize;
pub type LRESULT = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type BOOL = i32;

pub type HWND = isize;
pub type HICON = isize;
pub type HMENU = isize;
pub type HGLOBAL = isize;

pub type HookProc = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;
pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
pub type CtrlHandler = unsafe extern "system" fn(u32) -> BOOL;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct MSLLHOOKSTRUCT {
    pub pt: POINT,
    pub mouseData: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: u32,
    pub scanCode: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: isize,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WndProc,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: isize,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: isize,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HICON,
}

#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cbSize: u32,
    pub hWnd: HWND,
    pub uID: u32,
    pub uFlags: u32,
    pub uCallbackMessage: u32,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: u32,
    pub dwStateMask: u32,
    pub szInfo: [u16; 256],
    pub uVersion: u32,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: u32,
    pub guidItem: [u32; 4],
    pub hBalloonIcon: HICON,
}

const _: () = assert!(std::mem::size_of::<NOTIFYICONDATAW>() == 976);
const _: () = assert!(std::mem::size_of::<WNDCLASSEXW>() == 80);

pub const WH_KEYBOARD_LL: i32 = 13;
pub const WH_MOUSE_LL: i32 = 14;
pub const HC_ACTION: i32 = 0;

pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_XBUTTONDOWN: u32 = 0x020B;
pub const WM_XBUTTONUP: u32 = 0x020C;
pub const WM_MOUSEHWHEEL: u32 = 0x020E;

pub const WM_NULL: u32 = 0x0000;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_CLIPBOARDUPDATE: u32 = 0x031D;
pub const WM_APP: u32 = 0x8000;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIF_MESSAGE: u32 = 0x01;
pub const NIF_ICON: u32 = 0x02;
pub const NIF_TIP: u32 = 0x04;

pub const MF_STRING: u32 = 0x0000;
pub const MF_GRAYED: u32 = 0x0001;
pub const MF_CHECKED: u32 = 0x0008;
pub const MF_SEPARATOR: u32 = 0x0800;
pub const TPM_RIGHTBUTTON: u32 = 0x0002;
pub const TPM_RETURNCMD: u32 = 0x0100;

pub const CF_UNICODETEXT: u32 = 13;
pub const CF_DIB: u32 = 8;
pub const GMEM_MOVEABLE: u32 = 0x0002;

pub const LLMHF_INJECTED: u32 = 0x01;
pub const LLKHF_EXTENDED: u32 = 0x01;
pub const LLKHF_INJECTED: u32 = 0x10;
pub const XBUTTON1: u16 = 1;

pub const VK_SCROLL: u32 = 0x91;

pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
pub const SM_XVIRTUALSCREEN: i32 = 76;
pub const SM_YVIRTUALSCREEN: i32 = 77;
pub const SM_CXVIRTUALSCREEN: i32 = 78;
pub const SM_CYVIRTUALSCREEN: i32 = 79;

pub const SPI_SETCURSORS: u32 = 0x0057;

/// System cursor ids (OCR_*) replaced by a blank cursor in remote mode.
pub const SYSTEM_CURSORS: [u32; 14] = [
    32512, // NORMAL
    32513, // IBEAM
    32514, // WAIT
    32515, // CROSS
    32516, // UP
    32642, // SIZENWSE
    32643, // SIZENESW
    32644, // SIZEWE
    32645, // SIZENS
    32646, // SIZEALL
    32648, // NO
    32649, // HAND
    32650, // APPSTARTING
    32651, // HELP
];

pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
pub const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x2;

#[link(name = "user32")]
extern "system" {
    pub fn SetWindowsHookExW(idHook: i32, lpfn: HookProc, hmod: isize, dwThreadId: u32) -> HHOOK;
    pub fn UnhookWindowsHookEx(hhk: HHOOK) -> BOOL;
    pub fn CallNextHookEx(hhk: HHOOK, nCode: i32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn GetMessageW(msg: *mut MSG, hwnd: isize, min: u32, max: u32) -> BOOL;
    pub fn SetCursorPos(x: i32, y: i32) -> BOOL;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn CreateCursor(
        hinst: isize,
        xHotSpot: i32,
        yHotSpot: i32,
        width: i32,
        height: i32,
        andPlane: *const u8,
        xorPlane: *const u8,
    ) -> HCURSOR;
    pub fn SetSystemCursor(hcur: HCURSOR, id: u32) -> BOOL;
    pub fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, winIni: u32) -> BOOL;
    pub fn SetProcessDpiAwarenessContext(value: isize) -> BOOL;

    pub fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostMessageW(hwnd: HWND, msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub fn PostQuitMessage(code: i32);
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        exStyle: u32,
        className: *const u16,
        windowName: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: HMENU,
        instance: isize,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn GetCursorPos(point: *mut POINT) -> BOOL;

    pub fn CreateIcon(
        instance: isize,
        width: i32,
        height: i32,
        planes: u8,
        bitsPerPixel: u8,
        andBits: *const u8,
        xorBits: *const u8,
    ) -> HICON;

    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: u32, id: usize, text: *const u16) -> BOOL;
    pub fn TrackPopupMenu(
        menu: HMENU,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: HWND,
        rect: *const c_void,
    ) -> BOOL;
    pub fn DestroyMenu(menu: HMENU) -> BOOL;

    pub fn AddClipboardFormatListener(hwnd: HWND) -> BOOL;
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn CloseClipboard() -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn GetClipboardData(format: u32) -> isize;
    pub fn SetClipboardData(format: u32, data: isize) -> isize;
    pub fn IsClipboardFormatAvailable(format: u32) -> BOOL;
    pub fn RegisterClipboardFormatW(name: *const u16) -> u32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(message: u32, data: *const NOTIFYICONDATAW) -> BOOL;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn SetConsoleCtrlHandler(handler: Option<CtrlHandler>, add: BOOL) -> BOOL;
    pub fn GetModuleHandleW(name: *const u16) -> isize;
    pub fn GlobalAlloc(flags: u32, bytes: usize) -> HGLOBAL;
    pub fn GlobalLock(mem: HGLOBAL) -> *mut c_void;
    pub fn GlobalUnlock(mem: HGLOBAL) -> BOOL;
    pub fn GlobalSize(mem: HGLOBAL) -> usize;
    pub fn GlobalFree(mem: HGLOBAL) -> HGLOBAL;
}

/// NUL-terminated UTF-16 copy of `s`, for `*const u16` parameters.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[link(name = "bcrypt")]
extern "system" {
    pub fn BCryptGenRandom(alg: isize, buffer: *mut u8, len: u32, flags: u32) -> i32;
}
