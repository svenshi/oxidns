// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use tokio::time::{Duration, timeout};

use super::*;
use crate::infra::network::listen;

#[test]
fn reply_targets_preserve_peers_and_only_scope_link_local_replies() {
    for (peer, local, expected_interface) in [
        ("[::ffff:192.0.2.2]:1234", "::ffff:192.0.2.1", 0),
        ("[fd00::2]:1234", "fd00::1", 0),
        ("[fe80::2%7]:1234", "fe80::1", 7),
    ] {
        let peer = peer.parse().unwrap();
        let target = UdpReplyTarget::new(peer, local.parse().unwrap(), 7).unwrap();
        assert_eq!(target.peer, peer);
        assert_eq!(target.outgoing_interface(), expected_interface);
        assert_eq!(
            target.local_ip.is_ipv4(),
            peer.ip().to_canonical().is_ipv4()
        );
    }
    for local in ["0.0.0.0", "224.0.0.1", "::1"] {
        assert!(
            UdpReplyTarget::new("127.0.0.1:1234".parse().unwrap(), local.parse().unwrap(), 0)
                .is_err()
        );
    }
    assert!(
        UdpReplyTarget::new(
            "[fe80::2]:1234".parse().unwrap(),
            "fe80::1".parse().unwrap(),
            0
        )
        .is_err()
    );
    let scoped_peer = UdpReplyTarget::new(
        "[fe80::2%7]:1234".parse().unwrap(),
        "fd00::1".parse().unwrap(),
        0,
    )
    .unwrap();
    assert_eq!(scoped_peer.outgoing_interface(), 7);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn reply_addresses_survive_reordered_concurrent_sends() {
    let raw = listen::build_udp_socket("[::]:0".parse().unwrap(), |_| {}).unwrap();
    let port = raw.local_addr().unwrap().port();
    let server = UdpReplySocket::new(UdpSocket::from_std(raw).unwrap()).unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let first_addr = SocketAddr::new("127.0.0.2".parse().unwrap(), port);
    let second_addr = SocketAddr::new("127.0.0.3".parse().unwrap(), port);
    let mut buffer = [0; 128];
    client.send_to(b"first", first_addr).await.unwrap();
    let (_, first) = timeout(Duration::from_secs(1), server.recv_from(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    client.send_to(b"second", second_addr).await.unwrap();
    let (_, second) = timeout(Duration::from_secs(1), server.recv_from(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    let (second_sent, first_sent) = tokio::join!(
        server.send_to(b"second", second),
        server.send_to(b"first", first)
    );
    second_sent.unwrap();
    first_sent.unwrap();
    let mut seen = [false; 2];
    for _ in 0..2 {
        let (len, peer) = timeout(Duration::from_secs(1), client.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        let index = match &buffer[..len] {
            b"first" => {
                assert_eq!(peer, first_addr);
                0
            }
            b"second" => {
                assert_eq!(peer, second_addr);
                1
            }
            other => panic!("Unexpected reply: {other:?}"),
        };
        assert!(!seen[index]);
        seen[index] = true;
    }
}

#[tokio::test]
async fn initialization_errors_require_explicit_binding_and_release_socket() {
    let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let err =
        UdpReplySocket::initialize(socket, |_| Err(io::ErrorKind::Unsupported.into())).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    assert!(err.to_string().contains("bind an explicit IP address"));
    let rebound = UdpSocket::bind(addr).await.unwrap();
    drop(rebound);
    let fixed = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    UdpReplySocket::initialize(fixed, |_| {
        panic!("Explicit binding needs no packet information")
    })
    .unwrap();
}

#[tokio::test]
async fn udp_reply_socket_preserves_destination_and_rejects_truncation() {
    for (listen_addr, client_addr, destination) in [
        ("0.0.0.0:0", "127.0.0.1:0", "127.0.0.1"),
        ("[::]:0", "127.0.0.1:0", "127.0.0.1"),
        ("[::]:0", "[::1]:0", "::1"),
        ("127.0.0.1:0", "127.0.0.1:0", "127.0.0.1"),
        ("[::1]:0", "[::1]:0", "::1"),
    ] {
        let raw = listen::build_udp_socket(listen_addr.parse().unwrap(), |_| {}).unwrap();
        let port = raw.local_addr().unwrap().port();
        let server = UdpReplySocket::new(UdpSocket::from_std(raw).unwrap()).unwrap();
        let destination = SocketAddr::new(destination.parse().unwrap(), port);
        let client = UdpSocket::bind(client_addr).await.unwrap();
        let mut buffer = [0; 128];
        client.send_to(b"query", destination).await.unwrap();
        let (len, target) = timeout(Duration::from_secs(1), server.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..len], b"query");
        assert_eq!(target.local_ip, destination.ip());
        assert_eq!(
            target.peer.ip().to_canonical(),
            client.local_addr().unwrap().ip()
        );
        server.send_to(b"reply", target).await.unwrap();
        let (len, source) = timeout(Duration::from_secs(1), client.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..len], b"reply");
        assert_eq!(source, destination);
        client.send_to(&[1; 256], destination).await.unwrap();
        assert!(
            timeout(Duration::from_secs(1), server.recv_from(&mut buffer))
                .await
                .unwrap()
                .is_err()
        );
        // Dropping a pending read must not consume the next datagram.
        assert!(
            timeout(Duration::from_millis(10), server.recv_from(&mut buffer))
                .await
                .is_err()
        );
        client
            .send_to(b"after cancellation", destination)
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_secs(1), server.recv_from(&mut buffer))
                .await
                .unwrap()
                .is_ok()
        );
    }
}
