// Exercises std::process on the libc-free fullrust target: PATH lookup, exit
// codes, stdout/stderr capture (output), env passing, cwd, stdin piping, and
// signal-terminated status. Spawns real Linux processes via fork+execve.
// Unmodified std; static, no libc.
use std::io::{Read, Write};
use std::process::{Command, Stdio};

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
    // --- exit status via PATH lookup (`true` / `false`) ---
    let ok = Command::new("true").status().expect("spawn true");
    check("`true` exits 0", ok.success() && ok.code() == Some(0));

    let no = Command::new("false").status().expect("spawn false");
    check("`false` exits 1", !no.success() && no.code() == Some(1));

    // --- stdout capture via output() ---
    let out = Command::new("echo").arg("hello fullrust").output().expect("echo");
    check("echo exit 0", out.status.success());
    check("echo stdout", out.stdout == b"hello fullrust\n");
    check("echo stderr empty", out.stderr.is_empty());

    // --- absolute-path program, custom exit code ---
    let code = Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 42")
        .status()
        .expect("sh exit 42");
    check("sh exit 42", code.code() == Some(42));

    // --- stderr capture + interleaving (read2 must not deadlock) ---
    let mixed = Command::new("/bin/sh")
        .arg("-c")
        .arg("echo to-out; echo to-err 1>&2")
        .output()
        .expect("sh mixed");
    check("captured stdout stream", mixed.stdout == b"to-out\n");
    check("captured stderr stream", mixed.stderr == b"to-err\n");

    // --- large output (exercises read2 looping past one buffer) ---
    let big = Command::new("/bin/sh")
        .arg("-c")
        .arg("seq 1 5000")
        .output()
        .expect("seq");
    let lines = big.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count();
    check("large stdout fully captured", lines == 5000);

    // --- environment passing ---
    let env_out = Command::new("/bin/sh")
        .arg("-c")
        .arg("echo $FULLRUST_TEST_VAR")
        .env("FULLRUST_TEST_VAR", "spawned-ok")
        .output()
        .expect("env sh");
    check("child sees custom env", env_out.stdout == b"spawned-ok\n");

    // --- env_clear drops inherited vars ---
    let cleared = Command::new("/bin/sh")
        .arg("-c")
        .arg("echo [${PATH:-empty}]")
        .env_clear()
        .env("PATH", "/bin:/usr/bin")
        .output()
        .expect("cleared sh");
    check("env_clear honored", cleared.stdout == b"[/bin:/usr/bin]\n");

    // --- working directory ---
    let cwd_out = Command::new("/bin/sh")
        .arg("-c")
        .arg("pwd")
        .current_dir("/tmp")
        .output()
        .expect("cwd sh");
    let pwd = String::from_utf8_lossy(&cwd_out.stdout);
    check("current_dir honored", pwd.trim_end() == "/tmp" || pwd.trim_end() == "/private/tmp");

    // --- stdin piping into a child ---
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn cat");
    child.stdin.take().unwrap().write_all(b"piped through cat\n").unwrap();
    let mut got = String::new();
    child.stdout.take().unwrap().read_to_string(&mut got).unwrap();
    check("stdin->stdout pipe", got == "piped through cat\n");
    check("cat waited ok", child.wait().unwrap().success());

    // --- signal-terminated child has no exit code ---
    let killed = Command::new("/bin/sh")
        .arg("-c")
        .arg("kill -9 $$")
        .status()
        .expect("self-kill");
    check("SIGKILL: no exit code", killed.code().is_none());

    // --- nonexistent program fails to spawn (or execs to 127) ---
    let missing = Command::new("this-program-does-not-exist-xyz").status();
    check(
        "missing program errors or 127",
        match missing {
            Err(_) => true,
            Ok(s) => s.code() == Some(127),
        },
    );

    // --- id() is a plausible pid ---
    let mut sleeper = Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 7")
        .spawn()
        .expect("spawn sleeper");
    check("child id nonzero", sleeper.id() > 1);
    check("wait returns code", sleeper.wait().unwrap().code() == Some(7));

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("\nALL PROCESS TESTS PASSED");
    } else {
        println!("\n{fails} PROCESS TEST(S) FAILED");
        std::process::exit(1);
    }
}
