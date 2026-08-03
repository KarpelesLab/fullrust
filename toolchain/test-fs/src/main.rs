// Exercises std::fs on the libc-free fullrust target end to end: create/write,
// open/read/seek, metadata (size + type + mtime), permissions, directory
// creation + listing, rename, symlink + read_link, hard link, canonicalize,
// copy, file locks, and recursive removal. Unmodified std; static, no libc.
// (file_lock was unstable through 1.88 but stabilized in 1.89 — no gate needed.)
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::exit;

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
    // Work inside a unique-ish scratch dir under /tmp.
    let base = format!("/tmp/fullrust-fs-test-{}", std::process::id());
    let root = Path::new(&base);
    let _ = fs::remove_dir_all(root);
    fs::create_dir(root).expect("create_dir root");
    check("create_dir", root.is_dir());

    // --- write + read round-trip ---
    let file = root.join("hello.txt");
    {
        let mut f = File::create(&file).expect("create");
        f.write_all(b"hello, fullrust fs!\n").expect("write");
        f.sync_all().expect("fsync");
    }
    let body = fs::read_to_string(&file).expect("read_to_string");
    check("write/read round-trip", body == "hello, fullrust fs!\n");

    // --- metadata ---
    let md = fs::metadata(&file).expect("metadata");
    check("metadata.size", md.len() == 20);
    check("metadata.is_file", md.is_file() && !md.is_dir());
    check("metadata.modified", md.modified().is_ok());

    // --- seek + partial read ---
    {
        let mut f = File::open(&file).expect("open");
        f.seek(SeekFrom::Start(7)).expect("seek");
        let mut buf = [0u8; 9];
        f.read_exact(&mut buf).expect("read_exact");
        check("seek + read", &buf == b"fullrust ");
        check("stream_position", f.stream_position().unwrap() == 16);
    }

    // --- append ---
    {
        let mut f = OpenOptions::new().append(true).open(&file).expect("open append");
        f.write_all(b"second line\n").expect("append write");
    }
    let grown = fs::metadata(&file).unwrap().len();
    check("append grew file", grown == 32);

    // --- create_new is exclusive ---
    let excl = OpenOptions::new().write(true).create_new(true).open(&file);
    check("create_new on existing -> AlreadyExists", matches!(&excl,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists));

    // --- permissions (readonly toggle) ---
    {
        let mut perm = fs::metadata(&file).unwrap().permissions();
        check("perm not readonly", !perm.readonly());
        perm.set_readonly(true);
        fs::set_permissions(&file, perm).expect("set readonly");
        check("perm now readonly", fs::metadata(&file).unwrap().permissions().readonly());
        // restore so cleanup can unlink freely
        let mut p2 = fs::metadata(&file).unwrap().permissions();
        p2.set_readonly(false);
        fs::set_permissions(&file, p2).unwrap();
    }

    // --- directory listing ---
    fs::create_dir(root.join("sub")).unwrap();
    File::create(root.join("sub/a.txt")).unwrap();
    File::create(root.join("sub/b.txt")).unwrap();
    let mut names: Vec<String> = fs::read_dir(root.join("sub"))
        .expect("read_dir")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    check("read_dir lists entries", names == ["a.txt", "b.txt"]);
    let ftype_ok = fs::read_dir(root.join("sub"))
        .unwrap()
        .all(|e| e.unwrap().file_type().unwrap().is_file());
    check("dir entry file_type", ftype_ok);

    // --- rename ---
    let renamed = root.join("renamed.txt");
    fs::rename(&file, &renamed).expect("rename");
    check("rename moved file", !file.exists() && renamed.exists());

    // --- symlink + read_link ---
    let link = root.join("link.txt");
    #[allow(deprecated)]
    fs::soft_link(&renamed, &link).expect("symlink");
    check("symlink_metadata is_symlink", fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    check("read_link target", fs::read_link(&link).unwrap() == renamed);
    check("metadata follows symlink", fs::metadata(&link).unwrap().is_file());

    // --- hard link ---
    let hard = root.join("hard.txt");
    fs::hard_link(&renamed, &hard).expect("hard_link");
    check("hard_link same size", fs::metadata(&hard).unwrap().len() == grown);

    // --- copy ---
    let copy_dst = root.join("copy.txt");
    let n = fs::copy(&renamed, &copy_dst).expect("copy");
    check("copy byte count", n == grown);
    check("copy content matches", fs::read(&copy_dst).unwrap() == fs::read(&renamed).unwrap());

    // --- canonicalize ---
    let canon = fs::canonicalize(&copy_dst).expect("canonicalize");
    check("canonicalize is absolute", canon.is_absolute() && canon.ends_with("copy.txt"));

    // --- file locking ---
    {
        let f = File::open(&renamed).unwrap();
        f.lock().expect("lock");
        f.unlock().expect("unlock");
        check("lock/unlock", true);
    }

    // --- recursive removal ---
    fs::remove_dir_all(root).expect("remove_dir_all");
    check("remove_dir_all", !root.exists());

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("\nALL FS TESTS PASSED");
    } else {
        println!("\n{fails} FS TEST(S) FAILED");
        exit(1);
    }
}
