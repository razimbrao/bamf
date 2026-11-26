//! Tiny logger: stderr, plus a file when [`to_file`] was called (the
//! Windows server runs without a console). `BAMF_DEBUG=1` enables debug
//! output.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static DEBUG: AtomicBool = AtomicBool::new(false);
static START: OnceLock<Instant> = OnceLock::new();
static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Also writes every line to `path`, truncating it first. Creates the
/// parent directory.
pub fn to_file(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path)?;
    *FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(file);
    Ok(())
}

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
    let line = format!("[{:>8.3}] {level:<5} {args}\n", t.as_secs_f64());
    // Never panic over logging (eprintln! would, without a stderr).
    let _ = std::io::stderr().write_all(line.as_bytes());
    if let Some(file) = FILE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        let _ = file.write_all(line.as_bytes());
    }
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
