//! Clipboard sync with the client over an encrypted TCP stream on the same
//! port as the UDP input channel. The client connects; the newest
//! connection wins.
//!
//! Threads: one accepts connections, one reads the current connection and
//! sets the local clipboard, one reads, converts and sends local changes.
//! The main (input) thread only signals that the clipboard changed.

use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use bamf_proto::stream::{read_frame, write_frame, EchoGuard, MAX_CLIP};
use bamf_proto::{Direction, Key, Message, Session};

use std::time::Duration;

use super::clipboard;
use super::tray;

struct Shared {
    /// Current connection, with an id so a dying reader only clears its own.
    writer: Mutex<Option<(u64, TcpStream)>>,
    /// Kept across connections, so its counter only grows.
    send: Mutex<Session>,
    guard: Mutex<EchoGuard>,
    changes: Mutex<Sender<()>>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn shared() -> &'static Shared {
    SHARED.get().expect("clipboard sync not started")
}

pub fn start(listen: &str, key: &Key) -> Result<(), String> {
    let listener =
        TcpListener::bind(listen).map_err(|e| format!("cannot listen on TCP {listen}: {e}"))?;
    let (tx, rx) = channel();
    SHARED
        .set(Shared {
            writer: Mutex::new(None),
            send: Mutex::new(Session::new(key, Direction::ServerToClientStream)),
            guard: Mutex::new(EchoGuard::default()),
            changes: Mutex::new(tx),
        })
        .map_err(|_| "clipboard sync already started".to_string())?;

    // Kept across connections too, so frames recorded from an older
    // connection cannot be replayed into a new one.
    let receive = Arc::new(Mutex::new(Session::new(
        key,
        Direction::ServerToClientStream,
    )));
    std::thread::spawn(move || accept_loop(listener, receive));
    std::thread::spawn(move || send_loop(rx));
    Ok(())
}

pub fn connected() -> bool {
    SHARED.get().is_some_and(|s| lock(&s.writer).is_some())
}

/// Called on the main thread when the local clipboard changed.
pub fn clipboard_changed() {
    if connected() {
        let _ = lock(&shared().changes).send(());
    }
}

/// Wait this long after a change before reading: the application that
/// just copied may still be using the clipboard (opening it right away
/// makes some of them fail).
const SETTLE: Duration = Duration::from_millis(100);

fn accept_loop(listener: TcpListener, receive: Arc<Mutex<Session>>) {
    for (id, stream) in listener.incoming().enumerate() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                bamf_proto::debug!("clipboard accept failed: {e}");
                continue;
            }
        };
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let _ = stream.set_nodelay(true);
        let reader = match stream.try_clone() {
            Ok(r) => r,
            Err(e) => {
                bamf_proto::warn!("clipboard connection from {peer}: {e}");
                continue;
            }
        };
        let id = id as u64;
        if let Some((_, old)) = lock(&shared().writer).replace((id, stream)) {
            // Unblocks the old reader, which is waiting on the old socket.
            let _ = old.shutdown(Shutdown::Both);
        }
        bamf_proto::info!("clipboard connected from {peer}");
        let receive = receive.clone();
        std::thread::spawn(move || read_loop(id, reader, receive));
    }
}

fn read_loop(id: u64, mut stream: TcpStream, receive: Arc<Mutex<Session>>) {
    loop {
        let msg = read_frame(&mut stream, &mut lock(&receive));
        match msg {
            Ok(Message::Clip { mime, data }) => {
                lock(&shared().guard).received(&mime, &data);
                match clipboard::write(tray::window(), &mime, &data) {
                    Ok(()) => {
                        bamf_proto::info!("clipboard <- client ({mime}, {} bytes)", data.len())
                    }
                    Err(e) => bamf_proto::warn!("cannot set the clipboard: {e}"),
                }
            }
            Ok(other) => {
                bamf_proto::debug!("unexpected message on the clipboard stream: {other:?}")
            }
            Err(e) => {
                bamf_proto::debug!("clipboard connection closed: {e}");
                break;
            }
        }
    }
    let mut writer = lock(&shared().writer);
    if writer.as_ref().is_some_and(|(current, _)| *current == id) {
        *writer = None;
        bamf_proto::info!("clipboard disconnected");
    }
}

fn send_loop(changes: Receiver<()>) {
    while changes.recv().is_ok() {
        std::thread::sleep(SETTLE);
        // Several quick changes: only the newest content matters.
        while changes.try_recv().is_ok() {}
        let Some(local) = clipboard::read(tray::window()) else {
            continue;
        };
        let (mime, data) = match local.into_wire() {
            Ok(wire) => wire,
            Err(e) => {
                bamf_proto::warn!("cannot convert the clipboard image: {e}");
                continue;
            }
        };
        if data.len() > MAX_CLIP {
            bamf_proto::warn!("clipboard content too large to sync ({} bytes)", data.len());
            continue;
        }
        if !lock(&shared().guard).should_send(mime, &data) {
            continue;
        }
        let mut writer = lock(&shared().writer);
        let Some((_, stream)) = writer.as_mut() else {
            continue;
        };
        let len = data.len();
        let msg = Message::Clip {
            mime: mime.to_string(),
            data,
        };
        match write_frame(stream, &mut lock(&shared().send), &msg) {
            Ok(()) => bamf_proto::info!("clipboard -> client ({mime}, {len} bytes)"),
            Err(e) => {
                bamf_proto::warn!("clipboard send failed: {e}");
                let _ = stream.shutdown(Shutdown::Both);
                *writer = None;
            }
        }
    }
}
