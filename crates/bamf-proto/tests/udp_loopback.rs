//! End-to-end check of the encrypted channel over real UDP sockets.
#![cfg(not(target_os = "wasi"))]

use std::net::UdpSocket;
use std::time::Duration;

use bamf_proto::{Direction, Key, KeyState, Message, Session};

#[test]
fn ping_and_state_over_localhost() {
    let key = Key::from_bytes([9; 32]);
    let server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    client_sock
        .connect(server_sock.local_addr().unwrap())
        .unwrap();
    for s in [&server_sock, &client_sock] {
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    }

    let mut server = Session::new(&key, Direction::ServerToClient);
    let mut client = Session::new(&key, Direction::ClientToServer);

    client_sock
        .send(&client.seal(&Message::Ping { paused: false }))
        .unwrap();
    let mut buf = [0u8; 1500];
    let (len, client_addr) = server_sock.recv_from(&mut buf).unwrap();
    assert_eq!(
        server.open(&buf[..len]).unwrap(),
        Message::Ping { paused: false }
    );

    let mut keys = KeyState::default();
    keys.set(42, true);
    let state = Message::State { keys, buttons: 1 };
    server_sock
        .send_to(&server.seal(&state), client_addr)
        .unwrap();
    let len = client_sock.recv(&mut buf).unwrap();
    assert_eq!(client.open(&buf[..len]).unwrap(), state);
}
