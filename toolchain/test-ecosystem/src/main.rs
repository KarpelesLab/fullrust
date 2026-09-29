// Ecosystem smoke test: the vendored forks (tokio/mio net, rustix, socket2) and
// std::os::unix, resolved fresh from crates.io through the image's [patch].
// Built by .github/workflows/action-selftest.yml via the composite action.
#![feature(peer_credentials_unix_socket)]
use std::os::unix::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let me = std::process::id() as i32;

    // tokio: multi-task TCP echo on the multi-thread runtime + a timer.
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut b = [0u8; 5];
                s.read_exact(&mut b).await.unwrap();
                s.write_all(&b).await.unwrap();
            });
        }
    });
    let mut tasks = Vec::new();
    for _ in 0..8 {
        tasks.push(tokio::spawn(async move {
            let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
            c.write_all(b"hello").await.unwrap();
            let mut b = [0u8; 5];
            c.read_exact(&mut b).await.unwrap();
            assert_eq!(&b, b"hello");
        }));
    }
    for t in tasks { t.await.unwrap(); }
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // rustix: typed raw syscalls (mmap/munmap, getpid).
    unsafe {
        use rustix::mm::{mmap_anonymous, munmap, MapFlags, ProtFlags};
        let p = mmap_anonymous(std::ptr::null_mut(), 4096, ProtFlags::READ | ProtFlags::WRITE, MapFlags::PRIVATE).unwrap();
        *(p as *mut u8) = 42;
        assert_eq!(*(p as *mut u8), 42);
        munmap(p, 4096).unwrap();
    }
    assert_eq!(rustix::process::getpid().as_raw_nonzero().get(), me);

    // socket2 peer_cred + std::os::unix peer_cred on Unix socketpairs.
    let (a, _b) = socket2::Socket::pair(socket2::Domain::UNIX, socket2::Type::STREAM, None).unwrap();
    let cred = a.peer_cred().unwrap();
    assert_eq!(cred.pid, Some(me));
    let (x, _y) = UnixStream::pair().unwrap();
    assert_eq!(x.peer_cred().unwrap().pid, Some(me));

    println!("ecosystem ok: tokio 8x echo, rustix mmap, socket2+std peer_cred pid={me}");
}
