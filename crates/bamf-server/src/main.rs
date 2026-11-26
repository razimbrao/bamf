//! bamf-server: captures the keyboard and mouse on Windows and teleports
//! them to the bamf client when the cursor crosses a screen edge.

// The core modules are only used by the Windows runtime, but they are
// compiled (and tested) everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod controller;
#[cfg_attr(not(windows), allow(dead_code))]
mod geometry;
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
    bamf-server [CONFIG]     run with CONFIG (default: bamf-server.toml)
    bamf-server keygen       print a new random pre-shared key
    bamf-server --help

Set BAMF_DEBUG=1 for verbose logs. Scroll Lock returns input to Windows.";

/// Runtime settings, read from the config file.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Settings {
    pub listen: String,
    pub key: Key,
    pub side: Side,
    pub remote_width: u32,
    pub remote_height: u32,
    pub mode: PointerMode,
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
        };
        if settings.remote_width < 2 || settings.remote_height < 2 {
            return Err("remote_width and remote_height must be at least 2".into());
        }
        Ok(settings)
    }
}

fn main() {
    bamf_proto::log::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("keygen") => keygen(),
        path => Config::load(path.unwrap_or(DEFAULT_CONFIG))
            .and_then(|c| Settings::from_config(&c))
            .and_then(run),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
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
