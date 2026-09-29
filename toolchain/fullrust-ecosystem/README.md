# fullrust ecosystem bundle

`target_os = "fullrust"` is a new OS: it is ABI-identical to x86-64 Linux, but
it is neither `cfg(unix)` nor `target_os = "linux"`. Most **pure-Rust** crates
need nothing (serde, rsurl, …). The friction is a small set of **gateway
crates** that either send any unknown OS to a stub/libc path or gate their
Linux code on `cfg(unix)` / `target_os = "linux"`. This bundle carries a
fullrust-aware copy of each, and the Docker image injects them automatically as
a `[patch.crates-io]`.

## Policy

- **Vendor only when unavoidable.** A crate is here only because upstream sends
  fullrust to a stub or libc path and offers no cfg, feature or rustflag to
  override that. If a crate works unmodified, it stays out.
- **Keep the diff minimal and mechanical.** Each fork routes fullrust through
  the crate's *own existing Linux code*. It does this with cfg broadenings
  (`unix` becomes `any(unix, target_os = "fullrust")`, and `target_os = "linux"`
  becomes `any(target_os = "linux", target_os = "fullrust")`), plus, where the
  crate calls `libc`, a small private raw-syscall shim. There are no parallel
  hand-written backends, so the fullrust API matches the Linux one and rebasing
  onto a new upstream release is cheap. `patches/fullrust-cfg.py` does the cfg
  rewrite.
- `patches/<crate>-<version>.patch` holds the diff against the pristine
  crates.io source. Each patch header explains why the fork is needed and lists
  its rules.

## Contents

| crate | dir | fork |
|-------|-----|------|
| `getrandom` 0.2.17 | `crates/getrandom` | small `fullrust.rs` backend: the raw `getrandom(2)` syscall |
| `getrandom` 0.4.3 | `crates/getrandom-0.4` | two one-line broadenings so the built-in `linux_raw` backend auto-selects |
| `rustix` 1.1.5 | `crates/rustix` | fullrust treated as linux in `build.rs`/`Cargo.toml`, plus cfg broadening → its own libc-free `linux_raw` backend |
| `mio` 1.2.3 | `crates/mio` | cfg broadening → mio's own Linux epoll/eventfd/UDS backend, plus a private 324-line raw-syscall `libc` shim |
| `tokio` 1.53.1 | `crates/tokio` | `fullrust-cfg.py` cfg broadening + a private 364-line `libc`/signal-registration shim; `features = ["full"]` |
| `socket2` 0.6.5 | `crates/socket2-0.6` | upstream's own `sys/unix.rs` Linux paths on a private 578-line `libc` shim, plus `Socket::peer_cred` |
| `socket2` 0.5.10 | `crates/socket2` | same design as 0.6 (the shim file is byte-identical) |

### getrandom (0.2 and 0.4)

This is the highest-leverage crate. `rand`, most of the crypto ecosystem,
`uuid`, `ahash` and anything else that needs seed entropy go through it.
0.2 has no backend for an unknown OS, so the fork adds a tiny raw-syscall
backend. 0.3/0.4 already ship a libc-free `linux_raw` backend that is
gated to linux/android, and two broadenings enable it. Both majors are common
(0.2 through `rand`; 0.4 through `uuid`/`tempfile`/`purecrypto`), so both are
bundled.

### rustix 1.1.5: typed syscalls

rustix's `build.rs` picks the libc backend for any `os != "linux"`, and no
cfg or feature overrides that. Its `Cargo.toml` pulls in `libc`/`errno` for
non-Linux targets. Neither `--cfg linux_raw` nor a spoofed `target_os` gets
pristine rustix to build. The fork treats fullrust as linux in `build.rs` and
`Cargo.toml` and broadens cfgs mechanically. Everything else is rustix's own
`linux_raw` backend, unmodified, so no libc and no errno end up in the build
graph.

**All features build**, including mm/process/fs/net/event/termios/pty/shm/
io_uring, and so does `no_std`. This gives typed, safe wrappers for `mmap`,
`ioctl`, `setsid`, `prctl`, `memfd`, `epoll`, `timerfd`, `inotify`, `eventfd`,
futex, vDSO `clock_gettime`, mount and more. The re-exports of
`std::os::unix::fs` traits (`rustix::fs::{FileExt, MetadataExt, …}`) are left
out on fullrust. Use `std::os::unix::fs` directly.

**rustix 0.38 is deliberately not vendored.** The bundle does not cover
crates that are still on 0.38.

### mio 1.2.3

mio sends an unknown OS to its non-functional `shell` stub, and no cfg selects
another backend. The fork routes fullrust through mio's own Linux code:
an edge-triggered epoll selector, an eventfd waker, TCP/UDP,
`net::{UnixStream, UnixListener, UnixDatagram}` (pathname, abstract and
unnamed addresses, `pair`), `pipe` including `From<ChildStdin/Stdout/Stderr>`,
and `SourceFd`. The ~15 syscalls, structs and constants those paths use come
from a private shim (`src/sys/unix/libc.rs`); the Unix-domain and child-pipe
parts sit on fullrust std's `std::os::unix`/`std::os::linux`, which match
Linux. There is no libc crate in the build graph. Diff: 495 changed lines, of
which 324 are the shim.

### tokio 1.53.1

Stock tokio builds on fullrust without `net`/`process`/`signal`, but all its
Unix code (fd impls, `net::unix`, the process reapers, the signal driver, fs
Unix extensions) is `cfg(unix)`/`target_os = "linux"`, and those paths call
`libc` and `signal-hook-registry`. The fork runs `patches/fullrust-cfg.py
--keep tokio_unstable` over `src/`, so fullrust compiles tokio's Linux code;
the unstable-only `io-uring`/`taskdump` (Linux-only dependencies) stay off.
The ~20 `libc` items those paths use (fcntl, fstat, getsockopt/`SO_PEERCRED`,
`pidfd_open`, signal numbers, …) come from a private shim
(`src/fullrust_libc.rs`), which also stands in for the two
`signal-hook-registry` items tokio uses: `register` installs one
`SA_RESTART | SA_SIGINFO` handler per signal with raw `rt_sigaction` (with an
`sa_restorer` trampoline), runs the registered actions in order and chains to a
previously installed handler, and `FORBIDDEN` is the same list. The shim has no
C `errno`, so tokio's 9 `io::Error::last_os_error()` sites after a libc call
become a `last_os_error!()` macro (unchanged elsewhere). Diff: 695 changed
lines, of which 364 are the shim; the rest is ~130 mechanical cfg lines and
~30 hand-edited lines.

**All of `features = ["full"]` works:** TCP/UDP/`AsyncFd`, `net::unix`
(`UnixStream`/`UnixListener`/`UnixDatagram`, abstract names via a leading NUL,
`peer_cred`, `unix::pipe`), `process` (piped stdin/stdout/stderr, `wait`/
`try_wait`/`kill`/`kill_on_drop`, the Linux pidfd reaper and the SIGCHLD
reaper/orphan queue), `signal` (`unix::signal(SignalKind)`, `ctrl_c`), the fs
Unix extensions (`mode`, `custom_flags`, `DirEntry::ino`, `symlink`, …), on
both multi-thread and current-thread runtimes. It needs the bundled mio and
socket2-0.6.

**Why no signal-hook-registry fork:** it is a `cfg(unix)` dependency of tokio
(so absent on fullrust), and it calls `libc::sigaction` itself, so using it
would mean vendoring and shimming a second crate. The ~130 lines in tokio's
shim cover what tokio needs. Crates that use `signal-hook`/`signal-hook-registry`
directly are therefore not supported.

### socket2 0.6.5 and 0.5.10

Recent tokio/mio use 0.6, and a large part of the ecosystem still uses 0.5.
Upstream's non-Windows backend is libc-based and gated on `cfg(unix)`. Both
forks compile upstream's own `src/sys/unix.rs` and take every
`target_os = "linux"` branch. The `libc` names come from a private
`src/sys/fullrust_libc.rs`, which holds Linux x86-64 types with compile-time
size/alignment checks, constants generated from the real `libc` crate, and inline
`syscall`s that return `-errno`. The cfg rewrites are generated by
`patches/fullrust-cfg.py`, and only a handful of edits are made by hand. (This
replaces the earlier hand-written ~1000-line `sys/fullrust.rs` backend of the
0.5 fork.)

The result is **the whole Linux `feature = "all"` API**, including `mss`,
`mark`, `cork`, `quickack`, `bind_device`, TCP congestion, `reuse_port`,
`freebind`, `original_dst`, BPF `attach_filter`, DCCP, vsock, multicast,
`sendfile`, `Socket::pair`, keepalive, Unix-domain sockets (pathname, abstract,
socketpair), `SockAddr::as_unix` and the six `From` conversions to and from
`std::os::unix::net::{UnixStream, UnixListener, UnixDatagram}` (these use
fullrust std's `std::os::unix`, so they are upstream code too). 0.5 also
re-exports `socket2::{sock_filter, sockaddr_storage}`, because its API names
libc types.

There is also one **fullrust-only extension**: `Socket::peer_cred() -> UCred`
(`SO_PEERCRED`). Upstream socket2 has no equivalent.

## Wiring (`[patch.crates-io]`)

The image generates `$CARGO_HOME/config.toml` with one entry per `crates/*`
directory. Each **key is the directory name** and each entry sets `package` to
the real crate name. That way two majors of the same crate coexist, and cargo
applies each to its own version range:

```toml
[patch.crates-io]
"getrandom"     = { path = "/opt/fullrust/ecosystem/crates/getrandom",     package = "getrandom" }
"getrandom-0.4" = { path = "/opt/fullrust/ecosystem/crates/getrandom-0.4", package = "getrandom" }
"mio"           = { path = "/opt/fullrust/ecosystem/crates/mio",           package = "mio" }
"rustix"        = { path = "/opt/fullrust/ecosystem/crates/rustix",        package = "rustix" }
"socket2"       = { path = "/opt/fullrust/ecosystem/crates/socket2",       package = "socket2" }
"socket2-0.6"   = { path = "/opt/fullrust/ecosystem/crates/socket2-0.6",   package = "socket2" }
"tokio"         = { path = "/opt/fullrust/ecosystem/crates/tokio",         package = "tokio" }
```

These renamed keys work in `.cargo/config.toml`, which is what the image uses,
so your `Cargo.toml` is never touched. Outside the image, put the same block in
your project's `.cargo/config.toml` (or the workspace-root `Cargo.toml`) with
paths into this directory, then build:

```console
RUSTC_BOOTSTRAP=1 cargo +fullrust-1.98 build --release --target x86_64-unknown-linux-fullrust
```

A patch for a crate you don't depend on is only a harmless "not used" warning.
If cargo ignores a patch because the lockfile pins another version, run
`cargo update -p <crate>`. To build against pristine upstream, set
`FULLRUST_NO_ECOSYSTEM=1` (docker) or `no-ecosystem: true` (action).

## Known limits

- `signal-hook`/`signal-hook-registry` are not bundled (see tokio above), so
  crates that use them directly don't build. tokio's own `signal` works.
- The bundle helps only crates that go *through* these gateways. Crates that
  gate their **own** OS code on `cfg(unix)` stay unsupported until they get
  their own cfg broadening. This happens even when they depend on rustix.
  Examples: `tempfile` (`tempfile()`/`NamedTempFile`), `memmap2`,
  `polling`/`async-io`, `io-lifetimes`/`cap-std`, `procfs`.
- `parking_lot` builds, but on fullrust it uses a spinning thread parker.
- Crates you own don't belong here. Fix them at the source. For example,
  purecrypto carries a fullrust `OsRng` gated on `target_os = "fullrust"`.

## Verified

Each fork is exercised by a static, libc-free probe (no `PT_INTERP`, no
`NEEDED`) on several toolchains:

- **getrandom**: `rand` 0.8 with real per-run entropy. `fstool` (getrandom 0.4
  + purecrypto) builds and writes and reads an ext4 image.
- **rustix** (1.88, 1.95): mmap/mprotect/madvise, ioctl, fcntl/dup2,
  pgrp/setpgid, prctl, sched, futex, statx, memfd, inotify, eventfd+epoll, vDSO
  clock, timerfd, uname, getrandom, auxv, socketpair, pty, shm,
  `io_uring_setup`, mount.
- **mio** (1.88/1.90/1.94/1.95): TCP accept/read/write/refused through `Poll`,
  UDP (v4/v6), cross-thread `Waker`, pipe + `SourceFd`, poll timeouts. (1.88,
  1.98): Unix stream echo over a pathname and an abstract address, `pair`,
  datagrams (bound/unbound/pair), `From<ChildStdin/Stdout>` pipes to `cat`.
- **tokio** (1.90/1.94/1.95): multi-thread runtime, an 8-client TCP echo
  server over `JoinSet`, `TcpSocket` options, UDP, timers, mpsc/oneshot/
  `select!`, `spawn_blocking`, `AsyncFd`, fs, stdout. (1.88, 1.98, `full`):
  `sh -c 'echo hi; exit 3'` with piped stdout (exit code 3), stdin → `cat` →
  stdout, stderr, `kill` + `wait` (SIGKILL), concurrent waits, a `kill_on_drop`
  child reaped by the orphan queue, all both with the pidfd reaper and with it
  forced off (SIGCHLD reaper); self-sent SIGUSR1 on two streams, SIGHUP, SIGUSR2
  on a current-thread runtime and `ctrl_c` via self-SIGINT; Unix stream/datagram
  echo over pathnames and abstract names, `peer_cred`, `unix::pipe`,
  `DirEntry::ino`. `features = ["full"]` and 11 smaller feature sets build.
- **socket2 0.6/0.5** (1.88, 1.94/1.95): TCP, UDP, Unix
  pathname/abstract/socketpair, `peer_cred` matching getpid/getuid/getgid, ~70
  Linux socket options, sendfile, BPF, and loopback multicast. The output is
  identical to the same probe on `x86_64-unknown-linux-gnu` with pristine
  socket2. (1.88, 1.98): the `std::os::unix::net` ⇄ `Socket` conversions both
  ways and `SockAddr::as_unix`.
- CI (`toolchain/test-ecosystem`, through the action on the 1.88 and 1.98
  images): tokio TCP echo, process, signal and Unix sockets, rustix, socket2
  and std `peer_cred`.
