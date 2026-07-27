# fullrust ecosystem bundle

`target_os = "fullrust"` is a brand-new OS, so crates that gate on `cfg(unix)`
or an explicit `target_os` list don't recognize it. Most **pure-Rust** crates
need nothing (serde, and rsurl built with zero changes); the friction is a small
set of **gateway crates** that many others funnel through. This bundle carries a
fullrust-aware fork of each, so a consumer opts in with a `[patch.crates-io]`
block instead of discovering and patching them one compile-error at a time.

## Contents

| crate | version | what the fork adds |
|-------|---------|--------------------|
| `getrandom` | 0.2.17 | a `#[cfg(target_os = "fullrust")]` backend issuing the raw Linux `getrandom(2)` syscall (no libc) — see `crates/getrandom/src/fullrust.rs` |
| `getrandom` | 0.4.3 (`crates/getrandom-0.4/`) | two one-line broadenings so 0.3/0.4's built-in **`linux_raw`** raw-syscall backend auto-selects on fullrust (no fork of the backend itself, no rustflags) |
| `socket2` | 0.5.10 | a full raw-syscall `sys/fullrust.rs` backend (socket/bind/connect/accept/get-setsockopt/send/recv/…) + 3 one-line `#[cfg(unix)]` → `#[cfg(any(unix, target_os = "fullrust"))]` broadenings in shared code |

**getrandom 0.2 vs 0.4:** both majors are in the wild (0.2 via `rand`; 0.4 via
`uuid`/`tempfile`/`purecrypto`). They coexist in one `[patch.crates-io]` via
renamed keys — the image's baked config emits
`"getrandom-0.4" = { path = …, package = "getrandom" }` alongside the 0.2 entry,
so cargo applies each to its own version range. (0.4 got easy: getrandom 0.3+
already ships a libc-free raw-syscall backend; it was just gated to
linux/android.)

`getrandom` is the single highest-leverage crate: `rand`, most of the crypto
ecosystem, `uuid`, `ahash`, and anything needing seed entropy funnel through it.
Teaching it fullrust unblocks all of them with no consumer code changes.

`socket2` is the fd-level socket gateway (used by `mio`, async runtimes, and many
network crates). Unlike getrandom it isn't a tiny shim: the backend re-implements
socket2's `sys` interface over raw Linux syscalls (mirroring `std::sys::net`),
because socket2's stock backend is a ~3300-line libc layer.

`feature = "all"` **compiles and works** for the portable extras: `Socket::pair`
(socketpair), `nonblocking()`, full TCP keepalive (interval/retries), and
**`sendfile`** (zero-copy file→socket, via a fullrust `impl Socket` block). What is
**not** implemented is the remaining long tail of platform methods socket2 defines
inside its `sys/unix.rs` `impl Socket` block — `mss`, `mark`, `cork`, `quickack`,
`device`, TCP congestion, BPF `attach_filter`, DCCP, vsock — which are simply
absent for fullrust (each needs porting into the fullrust `impl Socket` block; do
so on demand). The six `target_os = "linux"`-only methods (`ip_transparent`,
`multicast_all_v4/v6`) also stay gated out (they hard-reference `libc::` consts).
Everything an ordinary TCP/UDP client or server needs — plus zero-copy send — is
covered.

`patches/` holds the *diff of our changes only* (on top of the pristine
crates.io source), for review and for upstreaming the gate broadening.

## Use it in a project

Add to the **workspace root** `Cargo.toml` (patches only take effect at the
workspace root), pointing at this directory:

```toml
[patch.crates-io]
# getrandom 0.2 consumers (rand, …):
getrandom = { path = "/abs/…/fullrust-ecosystem/crates/getrandom" }
# getrandom 0.4 consumers (uuid, tempfile, purecrypto, …) — renamed key + package:
"getrandom-0.4" = { path = "/abs/…/fullrust-ecosystem/crates/getrandom-0.4", package = "getrandom" }
socket2 = { path = "/abs/…/fullrust-ecosystem/crates/socket2" }
```

Include only the entries whose crates your project actually depends on (an unused
patch is a harmless cargo warning).

Then build for the target as usual:

```console
RUSTC_BOOTSTRAP=1 cargo +fullrust-1.88 build --release \
  --target x86_64-unknown-linux-fullrust
```

If cargo reports the patch was "not used" because the lockfile pins a different
source, nudge it: `cargo update -p getrandom`.

## A note on your own crates (e.g. purecrypto)

Crates *you* own don't belong in this bundle — fix them at the source. purecrypto
already carries a fullrust `OsRng` (gated `cfg(all(feature = "std", target_os =
"fullrust"))`, reading `/dev/urandom`); the durable fix is to **publish** that
version so consumers (rsurl) pick it up via the normal version requirement. Until
then a consumer can path-patch it the same way:

```toml
[patch.crates-io]
purecrypto = { path = "/abs/path/to/purecrypto" }
```

## Verified

- `rand` (0.8) + `getrandom` (0.2.17): compile and run on a static, libc-free
  fullrust binary, with real per-run entropy from the kernel (`thread_rng`,
  `gen`, `shuffle`).
- `fstool` (a real ~7.8 MB CLI, getrandom 0.4 + purecrypto + all pure-Rust
  compression codecs, `readline` dropped): builds static + libc-free and runs —
  `create`d an ext4 image and read its superblock back.
- `socket2` (0.5.10): a full loopback TCP round-trip — socket/`SO_REUSEADDR`/bind/
  listen/getsockname → connect/`TCP_NODELAY`+getsockopt readback → accept/
  getpeername → send/recv echo — plus `feature = "all"`: `Socket::pair`
  (socketpair) round-trip, `nonblocking()` toggle, and `sendfile` (zero-copy a
  4 KiB file to a socket, full + partial). Static and libc-free.
