//! Local control socket (`$XDG_RUNTIME_DIR/bamf.sock`), non-blocking and
//! polled from the main loop. See [`crate::control`] for the protocol.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use crate::control::{self, Command};

/// Generous: a clipboard image travels base64-encoded on one line.
const MAX_LINE: usize = 64 * 1024 * 1024;

pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("bamf.sock"),
        None => {
            // SAFETY: getuid has no preconditions.
            let uid = unsafe { getuid() };
            PathBuf::from(format!("/tmp/bamf-{uid}.sock"))
        }
    }
}

extern "C" {
    fn getuid() -> u32;
}

struct Conn {
    stream: UnixStream,
    inbuf: Vec<u8>,
    outbuf: Vec<u8>,
}

pub struct Control {
    listener: UnixListener,
    path: PathBuf,
    conns: Vec<Conn>,
}

impl Control {
    pub fn bind() -> Result<Self, String> {
        let path = socket_path();
        if UnixStream::connect(&path).is_ok() {
            return Err(format!(
                "another bamf-client is already running ({})",
                path.display()
            ));
        }
        let _ = std::fs::remove_file(&path); // stale socket from a crash
        let listener = UnixListener::bind(&path)
            .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        // Only this user may control the client.
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Self {
            listener,
            path,
            conns: Vec::new(),
        })
    }

    /// Accepts new connections and returns the commands received since the
    /// last call. `greeting` is sent to every new connection.
    pub fn poll(&mut self, greeting: &str) -> Vec<Command> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(true).is_ok() {
                        bamf_proto::debug!("control connection opened");
                        self.conns.push(Conn {
                            stream,
                            inbuf: Vec::new(),
                            outbuf: greeting.as_bytes().to_vec(),
                        });
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    bamf_proto::warn!("control accept failed: {e}");
                    break;
                }
            }
        }

        let mut commands = Vec::new();
        self.conns.retain_mut(|conn| {
            let alive = read_lines(conn, &mut commands) && flush(conn);
            if !alive {
                bamf_proto::debug!("control connection closed");
            }
            alive
        });
        commands
    }

    /// Queues `line` for every connection (sent on the next `poll`).
    pub fn broadcast(&mut self, line: &str) {
        for conn in &mut self.conns {
            conn.outbuf.extend_from_slice(line.as_bytes());
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads what is available; false when the peer is gone or misbehaves.
fn read_lines(conn: &mut Conn, commands: &mut Vec<Command>) -> bool {
    let mut buf = [0u8; 64 * 1024];
    loop {
        match conn.stream.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => conn.inbuf.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    while let Some(end) = conn.inbuf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = conn.inbuf.drain(..=end).collect();
        match std::str::from_utf8(&line)
            .map_err(|e| e.to_string())
            .and_then(control::parse)
        {
            Ok(cmd) => commands.push(cmd),
            Err(e) => bamf_proto::warn!("control: {e}"),
        }
    }
    conn.inbuf.len() <= MAX_LINE
}

fn flush(conn: &mut Conn) -> bool {
    while !conn.outbuf.is_empty() {
        match conn.stream.write(&conn.outbuf) {
            Ok(0) => return false,
            Ok(n) => {
                conn.outbuf.drain(..n);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    // A peer that never reads must not grow the buffer forever.
    conn.outbuf.len() <= MAX_LINE * 2
}

/// `bamf-client pause|resume|toggle|status`: sends one command to the
/// running client and prints its status.
pub fn send_command(command: &str) -> Result<(), String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("bamf-client is not running ({}: {e})", path.display()))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    // The greeting is the status before the command.
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if command != "status" {
        stream
            .write_all(format!("{command}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        line.clear();
        // The client answers every state change with a new status line.
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
    }
    print!("{line}");
    Ok(())
}
