// Exercises std::os::unix / std::os::linux and std::os::fullrust::syscall on the
// fullrust target (where cfg(unix) is false, yet the unix extension API exists
// with Linux-identical signatures). The same source also builds for
// x86_64-unknown-linux-gnu, so results can be compared against stock std.
//
// Build: RUSTC_BOOTSTRAP=1 cargo +fullrust-<v> build --release \
//          --target x86_64-unknown-linux-fullrust
#![feature(peer_credentials_unix_socket, linux_pidfd, unix_socket_ancillary_data, unix_mkfifo, tcp_quickack)]
#![allow(stable_features)]

use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, IoSlice, IoSliceMut, Read, Write};
use std::os::linux::fs::MetadataExt as LinuxMetadataExt;
use std::os::linux::net::{SocketAddrExt, TcpStreamExt, UnixSocketExt};
use std::os::linux::process::{ChildExt as _, CommandExt as _};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{
    DirBuilderExt, DirEntryExt, FileExt, FileTypeExt, MetadataExt, OpenOptionsExt,
    PermissionsExt,
};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::{
    AncillaryData, SocketAddr, SocketAncillary, UnixDatagram, UnixListener, UnixStream,
};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::os::unix::thread::JoinHandleExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

static mut FAILS: u32 = 0;

fn check(name: &str, cond: bool) {
    if cond {
        println!("ok   {name}");
    } else {
        println!("FAIL {name}");
        unsafe { FAILS += 1 };
    }
}

// --- raw syscalls: std::os::fullrust::syscall on fullrust, asm on gnu --------
#[cfg(target_os = "fullrust")]
mod sc {
    pub use std::os::fullrust::syscall::nr::*;
    pub use std::os::fullrust::syscall::{
        syscall0, syscall1, syscall2, syscall3, syscall6, syscall_result,
    };
}
#[cfg(not(target_os = "fullrust"))]
#[allow(non_upper_case_globals, dead_code)]
mod sc {
    use std::arch::asm;
    pub const SYS_getpid: usize = 39;
    pub const SYS_gettid: usize = 186;
    pub const SYS_getuid: usize = 102;
    pub const SYS_getgid: usize = 104;
    pub const SYS_setsid: usize = 112;
    pub const SYS_getsid: usize = 124;
    pub const SYS_mmap: usize = 9;
    pub const SYS_munmap: usize = 11;
    pub const SYS_ioctl: usize = 16;
    pub unsafe fn syscall0(n: usize) -> isize {
        syscall6(n, 0, 0, 0, 0, 0, 0)
    }
    pub unsafe fn syscall1(n: usize, a: usize) -> isize {
        syscall6(n, a, 0, 0, 0, 0, 0)
    }
    pub unsafe fn syscall2(n: usize, a: usize, b: usize) -> isize {
        syscall6(n, a, b, 0, 0, 0, 0)
    }
    pub unsafe fn syscall3(n: usize, a: usize, b: usize, c: usize) -> isize {
        syscall6(n, a, b, c, 0, 0, 0)
    }
    pub unsafe fn syscall6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
        let r: isize;
        asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b, in("rdx") c,
             in("r10") d, in("r8") e, in("r9") f, lateout("rcx") _, lateout("r11") _, options(nostack));
        r
    }
    pub fn syscall_result(r: isize) -> std::io::Result<usize> {
        if (-4095..0).contains(&r) { Err(std::io::Error::from_raw_os_error(-r as i32)) } else { Ok(r as usize) }
    }
}

fn getuid() -> u32 {
    unsafe { sc::syscall0(sc::SYS_getuid) as u32 }
}
fn getgid() -> u32 {
    unsafe { sc::syscall0(sc::SYS_getgid) as u32 }
}

/// Fields of /proc/<pid>/stat after the `(comm)`: returns (pid, pgrp, session).
fn proc_stat_ids(stat: &str) -> Option<(i32, i32, i32)> {
    let pid: i32 = stat.split_whitespace().next()?.parse().ok()?;
    let rest = &stat[stat.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // rest: state ppid pgrp session ...
    Some((pid, f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?))
}

fn main() {
    // Child mode for the CommandExt::exec test: replace ourselves with echo.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("exec-child") {
        let err = Command::new("/bin/echo").arg("exec-ok").exec();
        eprintln!("exec failed: {err}");
        std::process::exit(3);
    }
    // Child mode for the process::exit regression check.
    if args.get(1).map(|s| s.as_str()) == Some("exit-child") {
        let code: i32 = args[2].parse().unwrap();
        std::process::exit(code);
    }

    let dir = std::env::temp_dir().join(format!("fullrust-osunix-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::DirBuilder::new().mode(0o755).create(&dir).unwrap();

    // ---------------------------------------------------------------- fs
    let path = dir.join("file");
    {
        let mut f = OpenOptions::new().write(true).create(true).mode(0o640).open(&path).unwrap();
        f.write_all(b"hello, positional world").unwrap();
    }
    let md = fs::metadata(&path).unwrap();
    check("OpenOptionsExt::mode + PermissionsExt::mode", md.permissions().mode() & 0o777 == 0o640);
    fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    check("PermissionsExt::from_mode/set_permissions",
          fs::metadata(&path).unwrap().permissions().mode() & 0o777 == 0o600);
    let mut p = md.permissions();
    p.set_mode(0o644);
    check("PermissionsExt::set_mode", p.mode() == 0o644);

    let md = fs::metadata(&path).unwrap();
    check("MetadataExt::uid == getuid", md.uid() == getuid());
    check("MetadataExt::gid == getgid", md.gid() == getgid());
    check("MetadataExt::ino != 0", md.ino() != 0);
    check("MetadataExt::mode S_IFREG", md.mode() & 0o170000 == 0o100000);
    check("MetadataExt::nlink == 1", md.nlink() == 1);
    check("MetadataExt::size", md.size() == 23);
    check("MetadataExt::dev != 0", md.dev() != 0);
    check("MetadataExt::blksize > 0", md.blksize() > 0);
    check("MetadataExt::mtime > 0", md.mtime() > 1_000_000_000);
    check("linux MetadataExt::st_ino == ino", md.st_ino() == md.ino());
    #[allow(deprecated)]
    let raw_size = md.as_raw_stat().st_size;
    check("linux MetadataExt::as_raw_stat().st_size", raw_size == 23);
    fs::hard_link(&path, dir.join("hard")).unwrap();
    check("MetadataExt::nlink == 2 after hard_link", fs::metadata(&path).unwrap().nlink() == 2);

    // FileExt
    let f = OpenOptions::new().read(true).write(true).open(&path).unwrap();
    let mut buf = [0u8; 10];
    let n = f.read_at(&mut buf, 7).unwrap();
    check("FileExt::read_at", &buf[..n] == b"positional");
    f.write_at(b"POSITIONAL", 7).unwrap();
    let mut s = String::new();
    File::open(&path).unwrap().read_to_string(&mut s).unwrap();
    check("FileExt::write_at", s == "hello, POSITIONAL world");
    f.write_all_at(b"HELLO", 0).unwrap();
    let mut b5 = [0u8; 5];
    f.read_exact_at(&mut b5, 0).unwrap();
    check("FileExt::write_all_at/read_exact_at", &b5 == b"HELLO");
    // Position must be untouched by positional I/O.
    let mut first = [0u8; 5];
    (&f).read_exact(&mut first).unwrap();
    check("FileExt leaves cursor alone", &first == b"HELLO");

    // symlink + custom_flags(O_NOFOLLOW)
    let link = dir.join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    check("symlink + read_link", fs::read_link(&link).unwrap() == path);
    check("symlink_metadata is_symlink", fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    const O_NOFOLLOW: i32 = 0o400000;
    let e = OpenOptions::new().read(true).custom_flags(O_NOFOLLOW).open(&link).unwrap_err();
    check("OpenOptionsExt::custom_flags(O_NOFOLLOW) -> ELOOP", e.raw_os_error() == Some(40));
    check("custom_flags(O_NOFOLLOW) on regular file opens",
          OpenOptions::new().read(true).custom_flags(O_NOFOLLOW).open(&path).is_ok());

    // FileTypeExt / mkfifo / DirEntryExt
    let fifo = dir.join("fifo");
    std::os::unix::fs::mkfifo(&fifo, Permissions::from_mode(0o600)).unwrap();
    check("mkfifo + FileTypeExt::is_fifo", fs::metadata(&fifo).unwrap().file_type().is_fifo());
    check("FileTypeExt::is_char_device(/dev/null)",
          fs::metadata("/dev/null").unwrap().file_type().is_char_device());
    check("FileTypeExt::is_block_device false", !fs::metadata(&path).unwrap().file_type().is_block_device());
    let ent = fs::read_dir(&dir).unwrap().map(|e| e.unwrap()).find(|e| e.file_name() == "file").unwrap();
    check("DirEntryExt::ino", ent.ino() == fs::metadata(&path).unwrap().ino());
    check("chown to self (no-op)", std::os::unix::fs::chown(&path, Some(getuid()), Some(getgid())).is_ok());
    check("lchown to self", std::os::unix::fs::lchown(&link, Some(getuid()), None).is_ok());
    check("fchown to self", std::os::unix::fs::fchown(&f, None, Some(getgid())).is_ok());
    check("OsStrExt::as_bytes", path.as_os_str().as_bytes().ends_with(b"/file"));

    // ---------------------------------------------------------------- net
    let (mut a, mut b) = UnixStream::pair().unwrap();
    a.write_all(b"ping").unwrap();
    let mut rb = [0u8; 4];
    b.read_exact(&mut rb).unwrap();
    check("UnixStream::pair write/read", &rb == b"ping");
    let cred = a.peer_cred().unwrap();
    check("UnixStream::peer_cred pid", cred.pid == Some(std::process::id() as i32));
    check("UnixStream::peer_cred uid/gid", cred.uid == getuid() && cred.gid == getgid());

    let sock_path = dir.join("sock");
    let listener = UnixListener::bind(&sock_path).unwrap();
    check("UnixListener::local_addr pathname",
          listener.local_addr().unwrap().as_pathname() == Some(sock_path.as_path()));
    let t = std::thread::spawn({
        let sock_path = sock_path.clone();
        move || {
            let mut c = UnixStream::connect(&sock_path).unwrap();
            c.write_all(b"over-path").unwrap();
            let mut back = String::new();
            c.read_to_string(&mut back).unwrap();
            back
        }
    });
    let (mut conn, peer) = listener.accept().unwrap();
    check("UnixListener::accept peer unnamed", peer.is_unnamed());
    let mut got = [0u8; 9];
    conn.read_exact(&mut got).unwrap();
    check("UnixListener bind/accept path read", &got == b"over-path");
    conn.write_all(b"pong").unwrap();
    drop(conn);
    check("UnixStream connect path round-trip", t.join().unwrap() == "pong");

    let abs_name = format!("fullrust-osunix-{}", std::process::id());
    let abs = SocketAddr::from_abstract_name(abs_name.as_bytes()).unwrap();
    let alistener = UnixListener::bind_addr(&abs).unwrap();
    check("abstract local_addr",
          alistener.local_addr().unwrap().as_abstract_name() == Some(abs_name.as_bytes()));
    let mut ac = UnixStream::connect_addr(&abs).unwrap();
    let (mut aconn, _) = alistener.accept().unwrap();
    ac.write_all(b"abstract").unwrap();
    let mut ab = [0u8; 8];
    aconn.read_exact(&mut ab).unwrap();
    check("abstract namespace bind/connect", &ab == b"abstract");

    // vectored + nonblocking + timeouts
    let (va, vb) = UnixStream::pair().unwrap();
    (&va).write_vectored(&[IoSlice::new(b"ab"), IoSlice::new(b"cd")]).unwrap();
    let (mut x, mut y) = ([0u8; 2], [0u8; 2]);
    let n = (&vb).read_vectored(&mut [IoSliceMut::new(&mut x), IoSliceMut::new(&mut y)]).unwrap();
    check("UnixStream vectored I/O", n == 4 && &x == b"ab" && &y == b"cd");
    vb.set_nonblocking(true).unwrap();
    check("UnixStream nonblocking WouldBlock",
          (&vb).read(&mut x).unwrap_err().kind() == io::ErrorKind::WouldBlock);
    va.set_read_timeout(Some(std::time::Duration::from_millis(250))).unwrap();
    check("UnixStream read_timeout", va.read_timeout().unwrap().is_some());

    // SCM_RIGHTS fd passing (ancillary data)
    let (sa, sb) = UnixStream::pair().unwrap();
    let passed = File::open(&path).unwrap();
    let mut abuf = [0u8; 64];
    let mut anc = SocketAncillary::new(&mut abuf);
    anc.add_fds(&[passed.as_raw_fd()]);
    sa.send_vectored_with_ancillary(&[IoSlice::new(b"fd")], &mut anc).unwrap();
    let mut rbuf = [0u8; 64];
    let mut ranc = SocketAncillary::new(&mut rbuf);
    let mut two = [0u8; 2];
    let n = sb.recv_vectored_with_ancillary(&mut [IoSliceMut::new(&mut two)], &mut ranc).unwrap();
    let mut got_fd = None;
    for m in ranc.messages() {
        if let Ok(AncillaryData::ScmRights(mut r)) = m {
            got_fd = r.next();
        }
    }
    let ok = match got_fd {
        Some(fd) => {
            let mut f = unsafe { File::from_raw_fd(fd) };
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            s.starts_with("HELLO")
        }
        None => false,
    };
    check("SCM_RIGHTS fd passing", n == 2 && ok);

    // linux socket extensions
    va.set_passcred(true).unwrap();
    check("linux UnixSocketExt::set_passcred/passcred", va.passcred().unwrap());
    let tl = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ts = std::net::TcpStream::connect(tl.local_addr().unwrap()).unwrap();
    ts.set_quickack(true).unwrap();
    check("linux TcpStreamExt::quickack", ts.quickack().is_ok());

    // UnixDatagram
    let d1p = dir.join("d1");
    let d2p = dir.join("d2");
    let d1 = UnixDatagram::bind(&d1p).unwrap();
    let d2 = UnixDatagram::bind(&d2p).unwrap();
    d1.send_to(b"dgram", &d2p).unwrap();
    let mut db = [0u8; 16];
    let (n, from) = d2.recv_from(&mut db).unwrap();
    check("UnixDatagram send_to/recv_from", &db[..n] == b"dgram" && from.as_pathname() == Some(d1p.as_path()));
    let (u1, u2) = UnixDatagram::pair().unwrap();
    u1.send(b"x").unwrap();
    check("UnixDatagram::pair", u2.recv(&mut db).unwrap() == 1 && db[0] == b'x');
    let _ = sb;

    // ---------------------------------------------------------------- process
    let my_pid = std::process::id();
    check("process::parent_id != 0", std::os::unix::process::parent_id() != 0);

    let out = Command::new("/bin/cat").arg("/proc/self/stat").process_group(0).output().unwrap();
    let ids = proc_stat_ids(&String::from_utf8_lossy(&out.stdout));
    check("CommandExt::process_group(0) -> pgrp == pid",
          matches!(ids, Some((pid, pgrp, _)) if pid == pgrp));

    let mut cmd = Command::new("/bin/cat");
    cmd.arg("/proc/self/stat");
    unsafe {
        cmd.pre_exec(|| {
            sc::syscall_result(sc::syscall0(sc::SYS_setsid)).map(drop)
        });
    }
    let out = cmd.output().unwrap();
    let ids = proc_stat_ids(&String::from_utf8_lossy(&out.stdout));
    check("CommandExt::pre_exec(setsid via raw syscall) -> sid == pid",
          matches!(ids, Some((pid, _, sid)) if pid == sid && pid as u32 != my_pid));

    let mut cmd = Command::new("/bin/true");
    unsafe {
        cmd.pre_exec(|| Err(io::Error::from_raw_os_error(1)));
    }
    let e = cmd.spawn().unwrap_err();
    check("pre_exec error propagates to spawn", e.raw_os_error() == Some(1));

    let e = Command::new("definitely-not-a-real-program-xyz").spawn().unwrap_err();
    check("spawn missing program -> NotFound", e.kind() == io::ErrorKind::NotFound);

    let st = Command::new("/bin/true").uid(getuid()).gid(getgid()).status().unwrap();
    check("CommandExt::uid/gid (self)", st.success());

    let out = Command::new("/bin/sh").arg0("custom-argv0").arg("-c").arg("echo $0").output().unwrap();
    check("CommandExt::arg0", String::from_utf8_lossy(&out.stdout).trim() == "custom-argv0");

    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    child.kill().unwrap();
    let st = child.wait().unwrap();
    check("ExitStatusExt::signal == SIGKILL", st.signal() == Some(9) && st.code().is_none());
    check("ExitStatusExt::from_raw/into_raw", ExitStatus::from_raw(0x0100).code() == Some(1)
          && ExitStatus::from_raw(0x0100).into_raw() == 0x0100);
    check("ExitStatus Display signal", format!("{st}") == "signal: 9 (SIGKILL)");

    let exe = std::env::current_exe().unwrap();
    // Regression: 1.95's new sys/exit.rs had no fullrust arm -> abort (SIGILL/127).
    for code in [0, 7, 42, 255] {
        let st = Command::new(&exe).args(["exit-child", &code.to_string()]).status().unwrap();
        check(&format!("process::exit({code}) -> exit status {code}"), st.code() == Some(code));
    }
    let out = Command::new(&exe).arg("exec-child").output().unwrap();
    check("CommandExt::exec", out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "exec-ok");

    let child = Command::new("/bin/sh").args(["-c", "exit 7"]).create_pidfd(true).spawn().unwrap();
    let ok = match child.pidfd() {
        Ok(pidfd) => pidfd.wait().map(|s| s.code() == Some(7)).unwrap_or(false),
        Err(_) => false,
    };
    check("linux CommandExt::create_pidfd + PidFd::wait", ok);

    // Stdio from an OwnedFd (os::unix::process From<OwnedFd> for Stdio)
    let (r, w) = io::pipe().unwrap();
    let st = Command::new("/bin/echo").arg("via-owned-fd").stdout(Stdio::from(OwnedFd::from(w))).status().unwrap();
    let mut s = String::new();
    let mut r = r;
    r.read_to_string(&mut s).unwrap();
    check("Stdio::from(OwnedFd)", st.success() && s.trim() == "via-owned-fd");

    // ---------------------------------------------------------------- thread
    let (tx, rx) = std::sync::mpsc::channel();
    let h = std::thread::spawn(move || {
        tx.send(unsafe { sc::syscall0(sc::SYS_gettid) } as u64).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
    });
    let tid = rx.recv().unwrap();
    let pt = h.as_pthread_t() as u64;
    #[cfg(target_os = "fullrust")]
    check("JoinHandleExt::as_pthread_t == child tid", pt == tid);
    #[cfg(not(target_os = "fullrust"))]
    check("JoinHandleExt::as_pthread_t nonzero", pt != 0 && tid != 0);
    h.join().unwrap();

    // ---------------------------------------------------------------- raw syscalls
    let pid = sc::syscall_result(unsafe { sc::syscall0(sc::SYS_getpid) }).unwrap();
    check("syscall0(getpid) == process::id", pid as u32 == my_pid);
    let len = 8192usize;
    let addr = sc::syscall_result(unsafe {
        sc::syscall6(sc::SYS_mmap, 0, len, 0x3 /*RW*/, 0x22 /*PRIVATE|ANON*/, -1isize as usize, 0)
    });
    let ok = match addr {
        Ok(a) => {
            let p = a as *mut u8;
            unsafe {
                p.write(0xAB);
                p.add(len - 1).write(0xCD);
                let v = (p.read(), p.add(len - 1).read());
                v == (0xAB, 0xCD)
                    && sc::syscall_result(sc::syscall2(sc::SYS_munmap, a, len)).is_ok()
            }
        }
        Err(_) => false,
    };
    check("syscall6(mmap) + syscall2(munmap)", ok);
    let (pr, mut pw) = io::pipe().unwrap();
    pw.write_all(b"12345").unwrap();
    let mut avail: i32 = 0;
    const FIONREAD: usize = 0x541B;
    let r = sc::syscall_result(unsafe {
        sc::syscall3(sc::SYS_ioctl, pr.as_raw_fd() as usize, FIONREAD, &mut avail as *mut i32 as usize)
    });
    check("syscall3(ioctl FIONREAD) on a pipe", r.is_ok() && avail == 5);
    let e = sc::syscall_result(unsafe { sc::syscall1(sc::SYS_munmap, 1) });
    check("syscall_result maps -errno to io::Error", e.is_err());
    let e = sc::syscall_result(unsafe { sc::syscall3(sc::SYS_ioctl, 999_999, FIONREAD, 0) }).unwrap_err();
    check("syscall_result EBADF", e.raw_os_error() == Some(9));

    // last_os_error is meaningful after a failing std-internal libc-style call
    let e = UnixStream::connect(dir.join("nope")).unwrap_err();
    check("UnixStream::connect missing -> NotFound", e.kind() == io::ErrorKind::NotFound);

    let _ = fs::remove_dir_all(&dir);
    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("ALL OK");
    } else {
        println!("{fails} FAILURE(S)");
        std::process::exit(1);
    }
    let _: PathBuf = dir;
}
