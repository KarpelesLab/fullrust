// Ecosystem smoke test: the vendored forks (tokio net/process/signal/net::unix,
// mio, rustix, socket2) and std::os::unix, resolved fresh from crates.io through
// the image's [patch]. Built by .github/workflows/action-selftest.yml via the
// composite action.
#![feature(peer_credentials_unix_socket)]
use std::os::fullrust::syscall::{self, nr};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixDatagram, UnixListener, UnixStream};
use tokio::process::Command;
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::timeout;

const T: Duration = Duration::from_secs(10);

/// Sends `sig` to this process with a raw kill(2).
fn raise(sig: i32) {
    unsafe { syscall::syscall2(nr::KILL, std::process::id() as usize, sig as usize) }.unwrap();
}

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
    tokio::time::sleep(Duration::from_millis(10)).await;

    // tokio::process: piped stdout + exit code, stdin -> cat, kill + wait.
    let out = Command::new("/bin/sh").args(["-c", "echo hi; exit 3"]).output().await.unwrap();
    assert_eq!((out.stdout.as_slice(), out.status.code()), (&b"hi\n"[..], Some(3)));
    let mut cat = Command::new("cat").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut sin = cat.stdin.take().unwrap();
    let w = tokio::spawn(async move { sin.write_all(b"through cat").await.unwrap() });
    let mut got = String::new();
    timeout(T, cat.stdout.take().unwrap().read_to_string(&mut got)).await.unwrap().unwrap();
    w.await.unwrap();
    assert_eq!(got, "through cat");
    assert!(timeout(T, cat.wait()).await.unwrap().unwrap().success());
    let mut sleeper = Command::new("sleep").arg("30").spawn().unwrap();
    sleeper.kill().await.unwrap();
    assert_eq!(timeout(T, sleeper.wait()).await.unwrap().unwrap().signal(), Some(9));

    // tokio::signal: self-SIGUSR1 on a signal stream, ctrl_c via self-SIGINT.
    let mut usr1 = signal(SignalKind::user_defined1()).unwrap();
    raise(10);
    timeout(T, usr1.recv()).await.expect("SIGUSR1 not delivered").unwrap();
    let cc = tokio::spawn(tokio::signal::ctrl_c());
    tokio::time::sleep(Duration::from_millis(50)).await;
    raise(2);
    timeout(T, cc).await.expect("ctrl_c not delivered").unwrap().unwrap();

    // tokio::net::unix: stream echo over a temp path and an abstract name,
    // datagram round trip, peer_cred.
    let dir = std::env::temp_dir().join(format!("ecosystem-probe-{me}"));
    std::fs::create_dir_all(&dir).unwrap();
    for name in [dir.join("s.sock").into_os_string(), format!("\0ecosystem-probe-{me}").into()] {
        let l = UnixListener::bind(&name).unwrap();
        let srv = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut b = [0u8; 4];
            s.read_exact(&mut b).await.unwrap();
            s.write_all(&b).await.unwrap();
        });
        let mut c = UnixStream::connect(&name).await.unwrap();
        assert_eq!(c.peer_cred().unwrap().pid(), Some(me));
        c.write_all(b"unix").await.unwrap();
        let mut b = [0u8; 4];
        timeout(T, c.read_exact(&mut b)).await.unwrap().unwrap();
        assert_eq!(&b, b"unix");
        srv.await.unwrap();
    }
    let d1 = UnixDatagram::bind(dir.join("d1.sock")).unwrap();
    let d2 = UnixDatagram::bind(dir.join("d2.sock")).unwrap();
    d2.send_to(b"dgram", dir.join("d1.sock")).await.unwrap();
    let mut b = [0u8; 8];
    let n = timeout(T, d1.recv(&mut b)).await.unwrap().unwrap();
    assert_eq!(&b[..n], b"dgram");
    std::fs::remove_dir_all(&dir).unwrap();

    // rustix: typed raw syscalls (mmap/munmap, getpid).
    unsafe {
        use rustix::mm::{mmap_anonymous, munmap, MapFlags, ProtFlags};
        let p = mmap_anonymous(std::ptr::null_mut(), 4096, ProtFlags::READ | ProtFlags::WRITE, MapFlags::PRIVATE).unwrap();
        *(p as *mut u8) = 42;
        assert_eq!(*(p as *mut u8), 42);
        munmap(p, 4096).unwrap();
    }
    assert_eq!(rustix::process::getpid().as_raw_nonzero().get(), me);

    // socket2 peer_cred + std::os::unix peer_cred, socket2 <-> std Unix conversions.
    let (a, _b) = socket2::Socket::pair(socket2::Domain::UNIX, socket2::Type::STREAM, None).unwrap();
    let cred = a.peer_cred().unwrap();
    assert_eq!(cred.pid, Some(me));
    let (x, _y) = StdUnixStream::pair().unwrap();
    assert_eq!(x.peer_cred().unwrap().pid, Some(me));
    let x: StdUnixStream = socket2::Socket::from(x).into();
    drop(x);

    println!(
        "ecosystem ok: tokio 8x TCP echo, process (exit 3, cat pipe, kill), signal (SIGUSR1, ctrl_c), \
         unix (path+abstract stream, dgram, peer_cred), rustix mmap, socket2+std peer_cred pid={me}"
    );
}
