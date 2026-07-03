// Exercises the std polish items wired up for fullrust: strerror-quality error
// messages, is_terminal, env split/join paths, home_dir, thread::set_name
// (prctl), vectored read/write (readv/writev), and process-wide SIGPIPE-ignore
// (a write to a closed peer returns BrokenPipe instead of killing us).
// Unmodified std; static, no libc.
#![feature(can_vector)]
use std::io::{IoSlice, IoSliceMut, IsTerminal, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

static mut FAILS: u32 = 0;

fn check(name: &str, cond: bool) {
    if cond {
        println!("ok   {name}");
    } else {
        println!("FAIL {name}");
        unsafe { FAILS += 1 };
    }
}

/// Parse the `SigIgn:` hex bitmask from the contents of a `/proc/<pid>/status`.
fn sigign_mask(status: &str) -> u64 {
    status
        .lines()
        .find_map(|l| l.strip_prefix("SigIgn:"))
        .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
        .unwrap_or(0)
}

fn main() {
    // --- error_string: io::Error Display now carries strerror-quality text ---
    let err = std::fs::File::open("/no/such/path/at/all-xyz").unwrap_err();
    let msg = err.to_string();
    check("error message has text", msg.contains("No such file or directory"));
    println!("     ENOENT -> {msg:?}");
    let eacces = std::fs::File::create("/proc/cpuinfo/nope").unwrap_err();
    check("error kind decoded", eacces.raw_os_error().is_some());

    // --- env split_paths / join_paths round-trip ---
    let joined = std::env::join_paths(["/usr/bin", "/bin", "/opt/x"]).expect("join");
    check("join_paths uses ':'", joined.to_str() == Some("/usr/bin:/bin:/opt/x"));
    let parts: Vec<PathBuf> = std::env::split_paths(&joined).collect();
    check(
        "split_paths round-trip",
        parts == [PathBuf::from("/usr/bin"), PathBuf::from("/bin"), PathBuf::from("/opt/x")],
    );
    check("join_paths rejects ':'", std::env::join_paths(["a:b"]).is_err());

    // --- home_dir from $HOME ---
    unsafe { std::env::set_var("HOME", "/home/fullrust-tester") };
    check(
        "home_dir reads $HOME",
        std::env::home_dir() == Some(PathBuf::from("/home/fullrust-tester")),
    );

    // --- is_terminal: a regular file is never a tty ---
    let tmp = format!("/tmp/fullrust-osextra-{}.bin", std::process::id());
    {
        let f = std::fs::File::create(&tmp).unwrap();
        check("File is not a terminal", !f.is_terminal());
    }
    // stdio is_terminal must at least return a bool without panicking.
    let _ = std::io::stdout().is_terminal();
    let _ = std::io::stdin().is_terminal();
    check("stdio is_terminal callable", true);

    // --- vectored write (writev) then vectored read (readv) ---
    {
        let mut f = std::fs::File::create(&tmp).unwrap();
        let bufs = [IoSlice::new(b"hello "), IoSlice::new(b"vectored "), IoSlice::new(b"world")];
        let n = f.write_vectored(&bufs).expect("write_vectored");
        check("write_vectored wrote >1 buf", n == 20);
        check("is_write_vectored true", f.is_write_vectored());
    }
    {
        let mut f = std::fs::File::open(&tmp).unwrap();
        let mut a = [0u8; 6];
        let mut b = [0u8; 9];
        let mut c = [0u8; 5];
        let mut bufs = [IoSliceMut::new(&mut a), IoSliceMut::new(&mut b), IoSliceMut::new(&mut c)];
        let n = f.read_vectored(&mut bufs).expect("read_vectored");
        check("read_vectored read all", n == 20);
        check("is_read_vectored true", f.is_read_vectored());
        check(
            "vectored content split correctly",
            &a == b"hello " && &b == b"vectored " && &c == b"world",
        );
        f.seek(SeekFrom::Start(0)).unwrap();
        let mut all = String::new();
        f.read_to_string(&mut all).unwrap();
        check("vectored bytes landed in order", all == "hello vectored world");
    }
    let _ = std::fs::remove_file(&tmp);

    // --- thread::set_name reaches the kernel (prctl); per-thread name lives at
    //     /proc/thread-self/comm (NOT /proc/self/comm, which is the main thread) ---
    let observed = std::thread::Builder::new()
        .name("frust-worker".to_string())
        .spawn(|| std::fs::read_to_string("/proc/thread-self/comm").unwrap_or_default())
        .unwrap()
        .join()
        .unwrap();
    check("set_name set thread comm", observed.trim_end() == "frust-worker");
    println!("     /proc/thread-self/comm -> {:?}", observed.trim_end());

    // --- SIGPIPE is ignored: writing to a closed peer must NOT kill us ---
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(addr).unwrap();
    let (server, _) = listener.accept().unwrap();
    drop(server); // peer goes away
    let mut reported_error = false;
    for _ in 0..1000 {
        match client.write(&[0u8; 4096]) {
            Ok(_) => continue,
            Err(e)
                if e.kind() == std::io::ErrorKind::BrokenPipe
                    || e.kind() == std::io::ErrorKind::ConnectionReset =>
            {
                reported_error = true;
                break;
            }
            Err(_) => break,
        }
    }
    // The point is we are still alive to make this assertion at all.
    check("write to dead peer didn't kill process", true);
    check("write to dead peer reported error", reported_error);

    // --- SIGPIPE disposition is honored properly, not blanket-ignored ---
    // SIGPIPE is signal 13 -> bit 12 (mask 0x1000) in the /proc SigIgn field.
    const SIGPIPE_BIT: u64 = 1 << 12;
    // We (the parent) ignore SIGPIPE by default, so our own SigIgn has the bit.
    let self_status = std::fs::read_to_string("/proc/self/status").unwrap();
    check("parent ignores SIGPIPE (default)", sigign_mask(&self_status) & SIGPIPE_BIT != 0);
    // A spawned child must get SIG_DFL restored (bit clear), so ordinary Unix
    // tools terminate on a broken pipe instead of inheriting our SIG_IGN.
    let child = std::process::Command::new("cat")
        .arg("/proc/self/status")
        .output()
        .expect("spawn cat");
    let child_status = String::from_utf8_lossy(&child.stdout);
    check("spawned child restores SIGPIPE default", sigign_mask(&child_status) & SIGPIPE_BIT == 0);

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("\nALL OSEXTRA TESTS PASSED");
    } else {
        println!("\n{fails} OSEXTRA TEST(S) FAILED");
        std::process::exit(1);
    }
}
