//! Clipboard sync with the server over an encrypted TCP stream (the same
//! host and port as the UDP channel). Runs on its own threads so large
//! images never delay input; the main loop talks to it through channels.

use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use bamf_proto::stream::{read_frame, write_frame};
use bamf_proto::{Direction, Key, Message, Session};

const RETRY: Duration = Duration::from_secs(2);

/// Clipboard content: MIME type and bytes.
pub type Clip = (String, Vec<u8>);

pub struct Sync {
    /// Local changes to send.
    pub outgoing: Sender<Clip>,
    /// Content received from the server.
    pub incoming: Receiver<Clip>,
}

pub fn start(server: String, key: Key) -> Sync {
    let (out_tx, out_rx) = channel();
    let (in_tx, in_rx) = channel();
    std::thread::spawn(move || run(server, key, out_rx, in_tx));
    Sync {
        outgoing: out_tx,
        incoming: in_rx,
    }
}

fn connect(server: &str) -> std::io::Result<TcpStream> {
    let addr = server
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::other("no address"))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn run(server: String, key: Key, outgoing: Receiver<Clip>, incoming: Sender<Clip>) {
    // Both sessions live across reconnections so counters only grow.
    let mut send = Session::new(&key, Direction::ClientToServerStream);
    let receive = Arc::new(std::sync::Mutex::new(Session::new(
        &key,
        Direction::ClientToServerStream,
    )));
    let mut warned = false;
    loop {
        let mut stream = match connect(&server) {
            Ok(s) => s,
            Err(e) => {
                if !warned {
                    bamf_proto::warn!(
                        "clipboard: cannot connect to TCP {server} ({e}); \
                         is TCP allowed in the server's firewall? Retrying."
                    );
                    warned = true;
                }
                // Drop changes made while offline; the next copy syncs.
                while outgoing.try_recv().is_ok() {}
                std::thread::sleep(RETRY);
                continue;
            }
        };
        warned = false;
        bamf_proto::info!("clipboard connected");

        let alive = Arc::new(AtomicBool::new(true));
        let reader = match stream.try_clone() {
            Ok(r) => r,
            Err(_) => continue,
        };
        {
            let (alive, receive, incoming) = (alive.clone(), receive.clone(), incoming.clone());
            std::thread::spawn(move || {
                read_loop(reader, &receive, &incoming);
                alive.store(false, Ordering::Relaxed);
            });
        }

        while alive.load(Ordering::Relaxed) {
            match outgoing.recv_timeout(Duration::from_millis(500)) {
                Ok(mut clip) => {
                    while let Ok(newer) = outgoing.try_recv() {
                        clip = newer;
                    }
                    let (mime, data) = clip;
                    let len = data.len();
                    let msg = Message::Clip { mime, data };
                    if let Err(e) = write_frame(&mut stream, &mut send, &msg) {
                        bamf_proto::warn!("clipboard send failed: {e}");
                        break;
                    }
                    if let Message::Clip { mime, .. } = &msg {
                        bamf_proto::info!("clipboard -> server ({mime}, {len} bytes)");
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let _ = stream.shutdown(Shutdown::Both);
        bamf_proto::info!("clipboard disconnected");
        std::thread::sleep(RETRY);
    }
}

fn read_loop(mut stream: TcpStream, receive: &std::sync::Mutex<Session>, incoming: &Sender<Clip>) {
    loop {
        let msg = {
            let mut session = receive.lock().unwrap_or_else(|p| p.into_inner());
            read_frame(&mut stream, &mut session)
        };
        match msg {
            Ok(Message::Clip { mime, data }) => {
                bamf_proto::info!("clipboard <- server ({mime}, {} bytes)", data.len());
                if incoming.send((mime, data)).is_err() {
                    return;
                }
            }
            Ok(other) => {
                bamf_proto::debug!("unexpected message on the clipboard stream: {other:?}")
            }
            Err(e) => {
                bamf_proto::debug!("clipboard stream closed: {e}");
                return;
            }
        }
    }
}
