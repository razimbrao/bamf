//! bamf-client: receives keyboard and mouse events from the bamf server and
//! injects them into Linux through uinput.

// Only used by the Linux runtime, but compiled (and tested) everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod control;
#[cfg(target_os = "linux")]
mod linux;
// Only used by the Linux runtime, but compiled (and tested) everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod state;

use bamf_proto::config::Config;
use bamf_proto::Key;

const DEFAULT_CONFIG: &str = "bamf-client.toml";

const USAGE: &str = "\
bamf-client — receive keyboard and mouse from a bamf server

USAGE:
    bamf-client [CONFIG]     run with CONFIG (default: bamf-client.toml)
    bamf-client keygen       print a new random pre-shared key
    bamf-client pause        pause the running client (input stays on the server)
    bamf-client resume       resume it
    bamf-client toggle       pause or resume
    bamf-client status       print the running client's status
    bamf-client --help

Set BAMF_DEBUG=1 for verbose logs.";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct Settings {
    /// `host:port` of the server.
    pub server: String,
    pub key: Key,
}

impl Settings {
    fn from_config(c: &Config) -> Result<Self, String> {
        let mut server = c.required("server")?.to_string();
        if !server.contains(':') {
            server = format!("{server}:{}", bamf_proto::DEFAULT_PORT);
        }
        Ok(Settings {
            server,
            key: Key::from_hex(c.required("key")?)?,
        })
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
        Some(cmd @ ("pause" | "resume" | "toggle" | "status")) => send_command(cmd),
        path => Config::load(path.unwrap_or(DEFAULT_CONFIG))
            .and_then(|c| Settings::from_config(&c))
            .and_then(run),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn keygen() -> Result<(), String> {
    println!("{}", linux::random_key()?.to_hex());
    Ok(())
}

#[cfg(target_os = "linux")]
fn run(settings: Settings) -> Result<(), String> {
    linux::run(settings)
}

#[cfg(target_os = "linux")]
fn send_command(cmd: &str) -> Result<(), String> {
    linux::send_command(cmd)
}

#[cfg(not(target_os = "linux"))]
fn send_command(_cmd: &str) -> Result<(), String> {
    Err("bamf-client only runs on Linux".into())
}

#[cfg(not(target_os = "linux"))]
fn keygen() -> Result<(), String> {
    Err("bamf-client only runs on Linux (use bamf-server keygen on Windows)".into())
}

#[cfg(not(target_os = "linux"))]
fn run(_settings: Settings) -> Result<(), String> {
    Err("bamf-client only runs on Linux".into())
}
