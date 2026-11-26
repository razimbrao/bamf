//! Tiny stderr logger. `BAMF_DEBUG=1` enables debug output.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static DEBUG: AtomicBool = AtomicBool::new(false);
static START: OnceLock<Instant> = OnceLock::new();

pub fn init() {
    START.get_or_init(Instant::now);
    let on = std::env::var("BAMF_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0");
    DEBUG.store(on, Ordering::Relaxed);
}

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

#[doc(hidden)]
pub fn write(level: &str, args: std::fmt::Arguments<'_>) {
    let t = START.get_or_init(Instant::now).elapsed();
    eprintln!("[{:>8.3}] {level:<5} {args}", t.as_secs_f64());
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write("INFO", format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::write("WARN", format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::debug_enabled() {
            $crate::log::write("DEBUG", format_args!($($arg)*))
        }
    };
}
