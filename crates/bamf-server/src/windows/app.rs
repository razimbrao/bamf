//! Running as a windowless app: output, errors and starting at login.
//!
//! The server is built for the Windows GUI subsystem, so starting at login
//! opens no console window. Started from a terminal it borrows that
//! terminal's console for its output; in both cases it also logs to
//! `%LOCALAPPDATA%\bamf\bamf-server.log`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::ffi::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "bamf";

static CONSOLE: AtomicBool = AtomicBool::new(false);

/// Attaches to the parent's console when there is one (started from a
/// terminal) and points stdout/stderr at it.
pub fn attach_console() {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        let conout = wide("CONOUT$");
        let handle = CreateFileW(
            conout.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            0,
        );
        if handle != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
            SetStdHandle(STD_ERROR_HANDLE, handle);
            CONSOLE.store(true, Ordering::Relaxed);
        }
    }
}

pub fn log_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("bamf").join("bamf-server.log")
}

/// Shows a fatal error: on the console when there is one, else in a
/// message box (nobody would see it otherwise).
pub fn report_error(message: &str) {
    bamf_proto::warn!("error: {message}");
    if !CONSOLE.load(Ordering::Relaxed) {
        let text = wide(&format!("{message}\n\nLog: {}", log_path().display()));
        let caption = wide("bamf");
        unsafe {
            MessageBoxW(0, text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR);
        }
    }
}

/// `bamf-server install CONFIG`: start this executable with CONFIG at
/// every login of the current user.
pub fn install(config: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let config = std::fs::canonicalize(config)
        .map_err(|e| format!("cannot find {}: {e}", config.display()))?;
    // canonicalize returns \\?\ paths, which the shell does not need.
    let strip = |p: &Path| {
        p.display()
            .to_string()
            .trim_start_matches(r"\\?\")
            .to_string()
    };
    let command = format!("\"{}\" \"{}\"", strip(&exe), strip(&config));
    let data = wide(&command);
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(RUN_VALUE).as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    if status != 0 {
        return Err(format!(
            "cannot write the Run registry key (error {status})"
        ));
    }
    println!("bamf-server will start at login:\n  {command}");
    Ok(())
}

/// `bamf-server uninstall`: stop starting at login.
pub fn uninstall() -> Result<(), String> {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    let status = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(RUN_VALUE).as_ptr(),
        )
    };
    match status {
        0 => println!("bamf-server will no longer start at login"),
        ERROR_FILE_NOT_FOUND => println!("bamf-server was not set to start at login"),
        _ => {
            return Err(format!(
                "cannot remove the Run registry key (error {status})"
            ))
        }
    }
    Ok(())
}
