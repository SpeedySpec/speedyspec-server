use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::fd::AsFd;
use std::path::PathBuf;

use speedyspec_server_handoff::{
    Channel, GenerationRole, PROTOCOL, SocketInfo, SocketKind, ToGeneration, ToKeeper,
};

fn hello(sockets: Vec<SocketInfo>) -> ToGeneration {
    ToGeneration::Hello {
        protocol: PROTOCOL,
        config: PathBuf::from("/etc/x.toml"),
        sockets,
        role: GenerationRole::Managed,
    }
}

#[test]
fn round_trips_every_message() {
    let (keeper, child) = Channel::pair().unwrap();
    let generation = Channel::from_fd(child).unwrap();
    let to_generation = vec![
        hello(vec![SocketInfo {
            kind: SocketKind::Tcp,
            address: "127.0.0.1:1".parse().unwrap(),
            tls: None,
            http3: false,
        }]),
        ToGeneration::StartUdp,
        ToGeneration::Drain,
        ToGeneration::Resume {
            failed: Some("1.2.3".parse().unwrap()),
        },
        ToGeneration::ReloadCerts,
        ToGeneration::Shutdown,
        ToGeneration::Control("{\"Status\":null}".to_string()),
    ];
    for message in &to_generation {
        keeper.send(message, &[]).unwrap();
        let (received, fds) = generation.recv::<ToGeneration>().unwrap().unwrap();
        assert_eq!(&received, message);
        assert!(fds.is_empty());
    }
    let to_keeper = vec![
        ToKeeper::Ready {
            version: "0.2.0".parse().unwrap(),
            protocol: PROTOCOL,
        },
        ToKeeper::UdpReleased,
        ToKeeper::Upgrade {
            version: "0.2.0".parse().unwrap(),
            binary: PathBuf::from("/x"),
        },
        ToKeeper::Reload,
        ToKeeper::ControlReply("{}".to_string()),
        ToKeeper::Exiting,
    ];
    for message in &to_keeper {
        generation.send(message, &[]).unwrap();
        assert_eq!(&keeper.recv::<ToKeeper>().unwrap().unwrap().0, message);
    }
}

#[test]
fn passes_listening_sockets() {
    let (keeper, child) = Channel::pair().unwrap();
    let generation = Channel::from_fd(child).unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let sockets = vec![
        SocketInfo {
            kind: SocketKind::Tcp,
            address: tcp.local_addr().unwrap(),
            tls: None,
            http3: false,
        },
        SocketInfo {
            kind: SocketKind::Udp,
            address: udp.local_addr().unwrap(),
            tls: None,
            http3: true,
        },
    ];
    keeper
        .send(&hello(sockets), &[tcp.as_fd(), udp.as_fd()])
        .unwrap();
    let (message, mut fds) = generation.recv::<ToGeneration>().unwrap().unwrap();
    assert!(matches!(message, ToGeneration::Hello { .. }));
    assert_eq!(fds.len(), 2);
    let received_udp = UdpSocket::from(fds.pop().unwrap());
    let received_tcp = TcpListener::from(fds.pop().unwrap());

    let mut client = TcpStream::connect(tcp.local_addr().unwrap()).unwrap();
    client.write_all(b"ping").unwrap();
    let (mut accepted, _) = received_tcp.accept().unwrap();
    let mut buf = [0u8; 4];
    accepted.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");

    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .send_to(b"dgram", udp.local_addr().unwrap())
        .unwrap();
    let mut buf = [0u8; 8];
    let (n, _) = received_udp.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"dgram");
}

#[test]
fn unknown_message_is_skipped() {
    let (keeper, child) = Channel::pair().unwrap();
    let generation = Channel::from_fd(child).unwrap();
    keeper.send_raw_line("{\"Future\":{\"x\":1}}").unwrap();
    keeper.send(&ToGeneration::Shutdown, &[]).unwrap();
    assert_eq!(
        generation.recv::<ToGeneration>().unwrap().unwrap().0,
        ToGeneration::Shutdown
    );
}

#[test]
fn eof_is_none() {
    let (keeper, child) = Channel::pair().unwrap();
    let generation = Channel::from_fd(child).unwrap();
    drop(keeper);
    assert!(generation.recv::<ToGeneration>().unwrap().is_none());
}

#[test]
fn fds_are_close_on_exec_by_default() {
    let (keeper, child) = Channel::pair().unwrap();
    let generation = Channel::from_fd(child).unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    keeper
        .send(&ToGeneration::StartUdp, &[tcp.as_fd()])
        .unwrap();
    let (_, fds) = generation.recv::<ToGeneration>().unwrap().unwrap();
    let flags = rustix::io::fcntl_getfd(&fds[0]).unwrap();
    assert!(flags.contains(rustix::io::FdFlags::CLOEXEC));
    assert!(
        rustix::io::fcntl_getfd(keeper.as_fd())
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
}

#[tokio::test]
async fn async_channel_round_trips_with_fds() {
    let (keeper, child) = Channel::pair().unwrap();
    let generation = Channel::from_fd(child).unwrap().into_async().unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    keeper.send(&ToGeneration::Drain, &[tcp.as_fd()]).unwrap();
    let (message, fds) = generation.recv::<ToGeneration>().await.unwrap().unwrap();
    assert_eq!((message, fds.len()), (ToGeneration::Drain, 1));
    generation.send(&ToKeeper::UdpReleased, &[]).await.unwrap();
    assert_eq!(
        keeper.recv::<ToKeeper>().unwrap().unwrap().0,
        ToKeeper::UdpReleased
    );
}
