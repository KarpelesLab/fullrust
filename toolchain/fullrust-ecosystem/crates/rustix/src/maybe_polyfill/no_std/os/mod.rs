#[cfg(any(any(unix, target_os = "fullrust"), target_os = "wasi"))]
pub mod fd;
#[cfg(windows)]
pub mod windows;
