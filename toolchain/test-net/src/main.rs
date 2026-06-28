// Exercises std::net on the libc-free fullrust target: TCP listener/accept,
// client connect + echo over loopback (with a real thread), socket addresses,
// socket options (nodelay/ttl), connect_timeout, UDP send_to/recv_from,
// non-blocking accept, and DNS (IP literal + "localhost" + optional real
// resolution). Unmodified std; static, no libc.
use std::io::{Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket,
};
use std::thread;
use std::time::Duration;

static mut FAILS: u32 = 0;

fn check(name: &str, cond: bool) {
    if cond {
        println!("ok   {name}");
    } else {
        println!("FAIL {name}");
        unsafe { FAILS += 1 };
    }
}

fn main() {
    // --- TCP echo over loopback ---
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    check("listener bound to loopback", addr.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST));
    check("listener got ephemeral port", addr.port() != 0);

    let server = thread::spawn(move || {
        let (mut sock, peer) = listener.accept().expect("accept");
        assert!(peer.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST));
        let mut buf = [0u8; 64];
        let n = sock.read(&mut buf).expect("server read");
        sock.write_all(&buf[..n]).expect("server write"); // echo
    });

    let mut client = TcpStream::connect(addr).expect("connect");
    check("peer_addr matches listener", client.peer_addr().unwrap() == addr);
    check("set_nodelay", client.set_nodelay(true).is_ok() && client.nodelay().unwrap());
    client.set_ttl(64).expect("set_ttl");
    check("ttl roundtrip", client.ttl().unwrap() == 64);

    client.write_all(b"echo me").expect("client write");
    let mut got = [0u8; 7];
    client.read_exact(&mut got).expect("client read");
    check("tcp echo round-trip", &got == b"echo me");
    server.join().unwrap();

    // --- connect_timeout reaches a live listener ---
    let l2 = TcpListener::bind("127.0.0.1:0").unwrap();
    let a2 = l2.local_addr().unwrap();
    let t2 = thread::spawn(move || {
        let _ = l2.accept();
    });
    let c2 = TcpStream::connect_timeout(&a2, Duration::from_secs(2));
    check("connect_timeout connects", c2.is_ok());
    drop(c2);
    t2.join().unwrap();

    // --- connect refused on a dead port surfaces an error ---
    let refused = TcpStream::connect("127.0.0.1:1"); // port 1: nothing listening
    check("connect refused errors", refused.is_err());

    // --- non-blocking accept returns WouldBlock when idle ---
    let l3 = TcpListener::bind("127.0.0.1:0").unwrap();
    l3.set_nonblocking(true).unwrap();
    match l3.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            check("nonblocking accept WouldBlock", true)
        }
        other => {
            println!("unexpected accept: {other:?}");
            check("nonblocking accept WouldBlock", false);
        }
    }

    // --- UDP datagram round-trip ---
    let a = UdpSocket::bind("127.0.0.1:0").expect("udp bind a");
    let b = UdpSocket::bind("127.0.0.1:0").expect("udp bind b");
    let a_addr = a.local_addr().unwrap();
    let b_addr = b.local_addr().unwrap();
    a.send_to(b"udp-payload", b_addr).expect("send_to");
    let mut ubuf = [0u8; 32];
    let (n, from) = b.recv_from(&mut ubuf).expect("recv_from");
    check("udp recv content", &ubuf[..n] == b"udp-payload");
    check("udp sender addr", from == a_addr);

    // --- UDP connect + send/recv ---
    b.connect(a_addr).expect("udp connect");
    a.connect(b_addr).expect("udp connect a");
    a.send(b"connected-udp").expect("udp send");
    let m = b.recv(&mut ubuf).expect("udp recv");
    check("udp connected recv", &ubuf[..m] == b"connected-udp");

    // --- UDP read timeout fires ---
    let c = UdpSocket::bind("127.0.0.1:0").unwrap();
    c.set_read_timeout(Some(Duration::from_millis(150))).unwrap();
    // The kernel rounds SO_RCVTIMEO up to a jiffy boundary, so the readback is
    // >= what we asked for (and close to it), not bit-identical.
    let stored = c.read_timeout().unwrap().unwrap();
    check(
        "udp read_timeout stored",
        stored >= Duration::from_millis(150) && stored < Duration::from_millis(200),
    );
    let start = std::time::Instant::now();
    let timed = c.recv(&mut ubuf);
    check("udp recv times out", timed.is_err() && start.elapsed() >= Duration::from_millis(100));

    // --- name resolution ---
    let mut ip_iter = "127.0.0.1:8080".to_socket_addrs().expect("ip literal");
    check("IP literal resolves", ip_iter.next() == Some(SocketAddr::from(([127, 0, 0, 1], 8080))));

    let local: Vec<SocketAddr> =
        ("localhost", 80u16).to_socket_addrs().expect("localhost").collect();
    check("localhost resolves to loopback", local.iter().any(|a| a.ip().is_loopback()));

    // Real DNS is best-effort: only assert it *worked* if it returned anything.
    match ("one.one.one.one", 443u16).to_socket_addrs() {
        Ok(addrs) => {
            let v: Vec<_> = addrs.collect();
            if v.is_empty() {
                println!("note: DNS returned no addresses (offline?)");
            } else {
                check("DNS resolves a hostname", !v.is_empty());
                println!("     resolved one.one.one.one -> {:?}", v.first().unwrap());
            }
        }
        Err(_) => println!("note: DNS lookup failed (offline?), skipping"),
    }

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("\nALL NET TESTS PASSED");
    } else {
        println!("\n{fails} NET TEST(S) FAILED");
        std::process::exit(1);
    }
}
