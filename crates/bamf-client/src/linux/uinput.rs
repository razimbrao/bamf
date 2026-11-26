//! Minimal uinput bindings: create virtual input devices and write events.
//!
//! Layouts and ioctl numbers come from `linux/uinput.h` and
//! `linux/input-event-codes.h` (x86_64 / aarch64, 64-bit `time_t`).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;

use std::ffi::{c_int, c_ulong};

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

// Event types.
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;
pub const SYN_REPORT: u16 = 0;

// Relative axes.
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_HWHEEL: u16 = 0x06;
pub const REL_WHEEL: u16 = 0x08;
pub const REL_WHEEL_HI_RES: u16 = 0x0b;
pub const REL_HWHEEL_HI_RES: u16 = 0x0c;

// Absolute axes.
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;

// Mouse buttons.
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;
pub const MOUSE_BUTTONS: [u16; 5] = [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA];

/// Highest "normal" keyboard key code (KEY_MICMUTE); button codes start at 0x100.
pub const KEY_LAST_KEYBOARD: u16 = 248;

const BUS_VIRTUAL: u16 = 0x06;

// ioctl request numbers: _IO('U', n) / _IOW('U', n, T).
const fn iow(nr: u32, size: u32) -> c_ulong {
    ((1u32 << 30) | (size << 16) | ((b'U' as u32) << 8) | nr) as c_ulong
}
const UI_DEV_CREATE: c_ulong = (b'U' as c_ulong) << 8 | 1;
const UI_DEV_DESTROY: c_ulong = (b'U' as c_ulong) << 8 | 2;
const UI_DEV_SETUP: c_ulong = iow(3, std::mem::size_of::<UinputSetup>() as u32);
const UI_ABS_SETUP: c_ulong = iow(4, std::mem::size_of::<UinputAbsSetup>() as u32);
const UI_SET_EVBIT: c_ulong = iow(100, 4);
const UI_SET_KEYBIT: c_ulong = iow(101, 4);
const UI_SET_RELBIT: c_ulong = iow(102, 4);
const UI_SET_ABSBIT: c_ulong = iow(103, 4);

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    absinfo: InputAbsInfo,
}

#[repr(C)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    kind: u16,
    code: u16,
    value: i32,
}

// Compile-time checks against the kernel ABI.
const _: () = assert!(std::mem::size_of::<UinputSetup>() == 92);
const _: () = assert!(std::mem::size_of::<UinputAbsSetup>() == 28);
const _: () = assert!(std::mem::size_of::<InputEvent>() == 24);

/// What a virtual device can do.
#[derive(Default)]
pub struct Capabilities {
    pub keys: Vec<u16>,
    pub rel: Vec<u16>,
    /// `(axis, max)`; minimum is always 0.
    pub abs: Vec<(u16, i32)>,
}

pub struct VirtualDevice {
    file: File,
    pending: Vec<u8>,
}

fn check(ret: c_int, what: &str) -> std::io::Result<()> {
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        Err(std::io::Error::new(err.kind(), format!("{what}: {err}")))
    } else {
        Ok(())
    }
}

impl VirtualDevice {
    pub fn create(name: &str, product: u16, caps: &Capabilities) -> std::io::Result<Self> {
        let file = OpenOptions::new().write(true).open("/dev/uinput")?;
        let fd = file.as_raw_fd();

        unsafe {
            let set = |req, value: u16, what| check(ioctl(fd, req, value as c_ulong), what);
            if !caps.keys.is_empty() {
                set(UI_SET_EVBIT, EV_KEY, "UI_SET_EVBIT")?;
                for &k in &caps.keys {
                    set(UI_SET_KEYBIT, k, "UI_SET_KEYBIT")?;
                }
            }
            if !caps.rel.is_empty() {
                set(UI_SET_EVBIT, EV_REL, "UI_SET_EVBIT")?;
                for &r in &caps.rel {
                    set(UI_SET_RELBIT, r, "UI_SET_RELBIT")?;
                }
            }
            if !caps.abs.is_empty() {
                set(UI_SET_EVBIT, EV_ABS, "UI_SET_EVBIT")?;
                for &(axis, max) in &caps.abs {
                    set(UI_SET_ABSBIT, axis, "UI_SET_ABSBIT")?;
                    let setup = UinputAbsSetup {
                        code: axis,
                        absinfo: InputAbsInfo {
                            value: 0,
                            minimum: 0,
                            maximum: max,
                            fuzz: 0,
                            flat: 0,
                            resolution: 0,
                        },
                    };
                    check(ioctl(fd, UI_ABS_SETUP, &setup as *const _), "UI_ABS_SETUP")?;
                }
            }

            let mut setup = UinputSetup {
                id: InputId {
                    bustype: BUS_VIRTUAL,
                    vendor: 0x0bad,
                    product,
                    version: 1,
                },
                name: [0; 80],
                ff_effects_max: 0,
            };
            let n = name.len().min(79);
            setup.name[..n].copy_from_slice(&name.as_bytes()[..n]);
            check(ioctl(fd, UI_DEV_SETUP, &setup as *const _), "UI_DEV_SETUP")?;
            check(ioctl(fd, UI_DEV_CREATE), "UI_DEV_CREATE")?;
        }

        Ok(Self {
            file,
            pending: Vec::with_capacity(24 * 8),
        })
    }

    /// Queues one event; call [`sync`](Self::sync) to deliver the batch.
    pub fn emit(&mut self, kind: u16, code: u16, value: i32) {
        let ev = InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            kind,
            code,
            value,
        };
        // SAFETY: InputEvent is a plain #[repr(C)] struct without padding.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &ev as *const InputEvent as *const u8,
                std::mem::size_of::<InputEvent>(),
            )
        };
        self.pending.extend_from_slice(bytes);
    }

    /// Appends SYN_REPORT and writes all queued events.
    pub fn sync(&mut self) -> std::io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.emit(EV_SYN, SYN_REPORT, 0);
        let result = self.file.write_all(&self.pending);
        self.pending.clear();
        result
    }
}

impl Drop for VirtualDevice {
    fn drop(&mut self) {
        unsafe {
            ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY);
        }
    }
}
