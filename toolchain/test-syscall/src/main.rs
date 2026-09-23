// Exercises the fullrust-specific std extensions for talking to the kernel
// directly: `std::os::fullrust::syscall` (raw syscalls + `nr`/`errno` tables)
// and `std::os::fullrust::process` (`CommandExt::exec`/`pre_exec`/`arg0`/
// `uid`/`gid`/`process_group`, `ExitStatusExt`, `parent_id`), plus spawn-time
// error reporting (a failed exec surfaces as `spawn` error). Static, no libc.
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::fullrust::process::{CommandExt, ExitStatusExt, parent_id};
use std::os::fullrust::syscall::{self, errno, nr};
use std::process::{Command, ExitStatus};
use std::ptr;

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
    // Re-exec'd helper modes (see the `exec` checks below).
    match std::env::args().nth(1).as_deref() {
        Some("--exec-child") => {
            let err = Command::new("/bin/sh").args(["-c", "echo exec-ok; exit 7"]).exec();
            eprintln!("exec returned: {err}");
            std::process::exit(99);
        }
        Some("--raw-execve") => {
            let argv = [c"echo".as_ptr(), c"raw-execve-ok".as_ptr(), ptr::null()];
            let envp: [*const std::ffi::c_char; 1] = [ptr::null()];
            let err = unsafe {
                syscall::syscall3(
                    nr::EXECVE,
                    c"/bin/echo".as_ptr() as usize,
                    argv.as_ptr() as usize,
                    envp.as_ptr() as usize,
                )
            }
            .unwrap_err();
            eprintln!("execve returned: {err}");
            std::process::exit(99);
        }
        _ => {}
    }

    // --- raw syscalls ---
    let pid = unsafe { syscall::syscall0(nr::GETPID) }.expect("getpid");
    check("syscall0(GETPID) == process::id()", pid as u32 == std::process::id());

    let ppid = unsafe { syscall::syscall0(nr::GETPPID) }.expect("getppid");
    check("parent_id() == GETPPID", parent_id() == ppid as u32);

    let ebadf = unsafe { syscall::syscall1(nr::CLOSE, 999_999) }.unwrap_err();
    check("CLOSE(bad fd) -> Err(EBADF)", ebadf.raw_os_error() == Some(errno::EBADF));

    let tmp = format!("/tmp/fullrust-syscall-{}.bin", std::process::id());
    let mut f = File::options().read(true).write(true).create(true).truncate(true).open(&tmp).unwrap();
    let msg = b"written by a raw syscall";
    let n = unsafe {
        syscall::syscall3(nr::WRITE, f.as_raw_fd() as usize, msg.as_ptr() as usize, msg.len())
    }
    .expect("write");
    check("syscall3(WRITE) wrote everything", n == msg.len());
    f.seek(SeekFrom::Start(0)).unwrap();
    let mut back = Vec::new();
    f.read_to_end(&mut back).unwrap();
    check("raw write visible through std::fs", back == msg);
    drop(f);
    let _ = std::fs::remove_file(&tmp);

    let enoent = File::open("/no/such/file-xyz").unwrap_err();
    check("errno::ENOENT matches io::Error", enoent.raw_os_error() == Some(errno::ENOENT));

    // --- spawn now reports exec failures as errors (like unix) ---
    let missing = Command::new("/no/such/program-xyz").status();
    check(
        "spawn of missing program -> Err(NotFound)",
        matches!(&missing, Err(e) if e.kind() == ErrorKind::NotFound),
    );
    let missing_path = Command::new("no-such-program-xyz").status();
    check(
        "PATH lookup miss -> Err(NotFound)",
        matches!(&missing_path, Err(e) if e.kind() == ErrorKind::NotFound),
    );

    // --- CommandExt::arg0 ---
    let out = Command::new("/bin/cat").arg0("my-cat").arg("/proc/self/cmdline").output().unwrap();
    check("arg0 sets argv[0]", out.stdout.starts_with(b"my-cat\0/proc/self/cmdline\0"));

    // --- CommandExt::pre_exec: runs in the child, can make raw syscalls ---
    let mut cmd = Command::new("pwd");
    unsafe {
        cmd.pre_exec(|| {
            syscall::syscall1(nr::CHDIR, c"/proc".as_ptr() as usize)?;
            Ok(())
        });
    }
    let out = cmd.output().unwrap();
    check("pre_exec chdir seen by child", out.stdout == b"/proc\n");

    let mut cmd = Command::new("true");
    unsafe {
        cmd.pre_exec(|| Err(std::io::Error::from_raw_os_error(errno::EXDEV)));
    }
    let r = cmd.status();
    check(
        "pre_exec error propagates to spawn",
        matches!(&r, Err(e) if e.raw_os_error() == Some(errno::EXDEV)),
    );

    // --- CommandExt::process_group(0): child leads a new group ---
    let out = Command::new("cat").arg("/proc/self/stat").process_group(0).output().unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    let child_pid = stat.split(' ').next().unwrap_or("").to_string();
    let pgrp = stat.rsplit(')').next().unwrap_or("").split_whitespace().nth(2).unwrap_or("");
    check("process_group(0) -> pgrp == pid", !child_pid.is_empty() && pgrp == child_pid);

    // --- CommandExt::uid / gid ---
    let uid = unsafe { syscall::syscall0(nr::GETUID) }.unwrap() as u32;
    let gid = unsafe { syscall::syscall0(nr::GETGID) }.unwrap() as u32;
    if uid == 0 {
        let out = Command::new("id").arg("-u").uid(65534).gid(65534).output().unwrap();
        check("uid(65534) as root", out.stdout == b"65534\n");
    } else {
        let out = Command::new("id").arg("-u").uid(uid).gid(gid).output().unwrap();
        check("uid(self) accepted", out.stdout == format!("{uid}\n").as_bytes());
        let r = Command::new("true").uid(0).status();
        check(
            "uid(0) as non-root -> Err(EPERM)",
            matches!(&r, Err(e) if e.raw_os_error() == Some(errno::EPERM)),
        );
    }

    // --- ExitStatusExt ---
    let st = Command::new("/bin/sh").args(["-c", "kill -9 $$"]).status().unwrap();
    check("signal() == Some(SIGKILL)", st.signal() == Some(9) && st.code().is_none());
    check("core_dumped() false", !st.core_dumped());
    check("into_raw() == 9", st.into_raw() == 9);
    let st = ExitStatus::from_raw(3 << 8);
    check("from_raw(3<<8).code() == 3", st.code() == Some(3) && st.signal().is_none());
    check("stopped_signal()", ExitStatus::from_raw(0x137f).stopped_signal() == Some(0x13));
    check("continued()", ExitStatus::from_raw(0xffff).continued());

    // --- CommandExt::exec: in-process failure returns the error ---
    let err = Command::new("/no/such/program-xyz").exec();
    check("exec of missing program -> NotFound", err.kind() == ErrorKind::NotFound);

    // --- CommandExt::exec / raw EXECVE replace the process image ---
    let me = std::env::current_exe().expect("current_exe");
    let out = Command::new(&me).arg("--exec-child").output().unwrap();
    check(
        "exec replaces the process",
        out.stdout == b"exec-ok\n" && out.status.code() == Some(7),
    );
    let out = Command::new(&me).arg("--raw-execve").output().unwrap();
    check(
        "raw syscall3(EXECVE) replaces the process",
        out.stdout == b"raw-execve-ok\n" && out.status.success(),
    );

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("ALL OK");
    } else {
        println!("{fails} FAILED");
        std::process::exit(1);
    }
}
