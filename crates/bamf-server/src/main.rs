//! bamf-server: captures the keyboard and mouse on Windows and teleports
//! them to the bamf client when the cursor crosses a screen edge.

// No console window, so it can start at login quietly; see windows/app.rs.
#![cfg_attr(windows, windows_subsystem = "windows")]

// The core modules are only used by the Windows runtime, but they are
// compiled (and tested) everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod controller;
#[cfg_attr(not(windows), allow(dead_code))]
mod geometry;
#[cfg_attr(not(windows), allow(dead_code))]
mod icon;
#[cfg_attr(not(windows), allow(dead_code))]
mod image;
#[cfg_attr(not(windows), allow(dead_code))]
mod keymap;
#[cfg(windows)]
mod windows;

use bamf_proto::config::Config;
use bamf_proto::{Key, DEFAULT_PORT};

use controller::PointerMode;
use geometry::Side;

const DEFAULT_CONFIG: &str = "bamf-server.toml";

const USAGE: &str = "\
bamf-server — share this computer's keyboard and mouse with a bamf client

USAGE:
    bamf-server [CONFIG]          run with CONFIG
    bamf-server keygen            print a new random pre-shared key
    bamf-server install [CONFIG]  start with CONFIG at every login
    bamf-server uninstall         stop starting at login
    bamf-server --help

CONFIG defaults to bamf-server.toml in the current directory, or else next
to the executable. Logs go to %LOCALAPPDATA%\\bamf\\bamf-server.log (and to
the terminal when started from one). Set BAMF_DEBUG=1 for verbose logs.
Scroll Lock returns input to Windows.";

/// The config file to use: the one given, else `bamf-server.toml` in the
/// current directory, else next to the executable (where it is when
/// started at login).
fn config_path(arg: Option<&str>) -> std::path::PathBuf {
    if let Some(path) = arg {
        return path.into();
    }
    let here = std::path::PathBuf::from(DEFAULT_CONFIG);
    if here.exists() {
        return here;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(DEFAULT_CONFIG)))
        .unwrap_or(here)
}

/// Runtime settings, read from the config file.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Settings {
    pub listen: String,
    pub key: Key,
    pub side: Side,
    pub remote_width: u32,
    pub remote_height: u32,
    pub mode: PointerMode,
    pub offset: Option<i32>,
}

impl Settings {
    fn from_config(c: &Config) -> Result<Self, String> {
        let settings = Settings {
            listen: c
                .str("listen")
                .map(str::to_string)
                .unwrap_or_else(|| format!("0.0.0.0:{DEFAULT_PORT}")),
            key: Key::from_hex(c.required("key")?)?,
            side: c.parse_or("remote_side", Side::Right)?,
            remote_width: c.parse_or("remote_width", 1920)?,
            remote_height: c.parse_or("remote_height", 1080)?,
            mode: c.parse_or("pointer_mode", PointerMode::Absolute)?,
            offset: c.parse_opt("remote_offset")?,
        };
        if settings.remote_width < 2 || settings.remote_height < 2 {
            return Err("remote_width and remote_height must be at least 2".into());
        }
        Ok(settings)
    }
}

fn main() {
    #[cfg(windows)]
    windows::app::attach_console();
    bamf_proto::log::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let result = match arg(0) {
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("keygen") => keygen(),
        Some("install") => install(&config_path(arg(1))),
        Some("uninstall") => uninstall(),
        path => {
            #[cfg(windows)]
            let _ = bamf_proto::log::to_file(&windows::app::log_path());
            Config::load(&config_path(path).to_string_lossy())
                .and_then(|c| Settings::from_config(&c))
                .and_then(run)
        }
    };
    if let Err(e) = result {
        report_error(&e);
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn report_error(message: &str) {
    windows::app::report_error(message);
}

#[cfg(not(windows))]
fn report_error(message: &str) {
    eprintln!("error: {message}");
}

#[cfg(windows)]
fn install(config: &std::path::Path) -> Result<(), String> {
    windows::app::install(config)
}

#[cfg(windows)]
fn uninstall() -> Result<(), String> {
    windows::app::uninstall()
}

#[cfg(not(windows))]
fn install(_config: &std::path::Path) -> Result<(), String> {
    Err("bamf-server only runs on Windows".into())
}

#[cfg(not(windows))]
fn uninstall() -> Result<(), String> {
    Err("bamf-server only runs on Windows".into())
}

#[cfg(windows)]
fn keygen() -> Result<(), String> {
    println!("{}", windows::random_key().to_hex());
    Ok(())
}

#[cfg(windows)]
fn run(settings: Settings) -> Result<(), String> {
    windows::run(settings)
}

#[cfg(not(windows))]
fn keygen() -> Result<(), String> {
    Err("bamf-server only runs on Windows (bamf-client keygen works on Linux)".into())
}

#[cfg(not(windows))]
fn run(_settings: Settings) -> Result<(), String> {
    Err("bamf-server only runs on Windows".into())
}
