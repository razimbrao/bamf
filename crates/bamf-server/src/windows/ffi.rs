//! Hand-written bindings for the few Win32 APIs bamf needs (x86_64).

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type HHOOK = isize;
pub type HCURSOR = isize;
pub type LRESULT = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type BOOL = i32;

pub type HookProc = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;
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
}

#[link(name = "kernel32")]
extern "system" {
    pub fn SetConsoleCtrlHandler(handler: Option<CtrlHandler>, add: BOOL) -> BOOL;
}

#[link(name = "bcrypt")]
extern "system" {
    pub fn BCryptGenRandom(alg: isize, buffer: *mut u8, len: u32, flags: u32) -> i32;
}
