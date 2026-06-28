// Exercises std::env on the libc-free target: vars from envp, set/remove,
// and current_dir/set_current_dir/current_exe. Unmodified std; static.
use std::env;

fn main() {
    // 1. Read an inherited var (PATH almost always exists; fall back to HOME).
    let path = env::var("PATH").or_else(|_| env::var("HOME"));
    println!("inherited PATH/HOME present: {}", path.is_ok());

    // 2. vars() iterates the whole environment.
    let n = env::vars().count();
    println!("env var count: {n} (>0: {})", n > 0);
    assert!(n > 0, "no environment variables seen");

    // 3. set_var / var / remove_var round-trip.
    unsafe { env::set_var("FULLRUST_TEST", "hello-libc-free") };
    assert_eq!(env::var("FULLRUST_TEST").unwrap(), "hello-libc-free");
    unsafe { env::set_var("FULLRUST_TEST", "updated") };
    assert_eq!(env::var("FULLRUST_TEST").unwrap(), "updated");
    unsafe { env::remove_var("FULLRUST_TEST") };
    assert!(env::var("FULLRUST_TEST").is_err());
    println!("set/update/remove round-trip: OK");

    // 4. current_dir, current_exe.
    let cwd = env::current_dir().expect("current_dir");
    println!("cwd = {}", cwd.display());
    assert!(cwd.is_absolute(), "cwd not absolute");
    let exe = env::current_exe().expect("current_exe");
    println!("exe = {}", exe.display());
    assert!(exe.is_absolute() && exe.to_string_lossy().contains("env-fullrust"));

    // 5. set_current_dir to /tmp then back.
    env::set_current_dir("/").expect("chdir /");
    assert_eq!(env::current_dir().unwrap().to_string_lossy(), "/");
    env::set_current_dir(&cwd).expect("chdir back");

    println!("OK: env vars + cwd + current_exe, libc-free");
}
