#!/usr/bin/env bash
# fullrust build wrapper.
#
# Produces libc-free, fully-static, pure-Rust binaries via one of two paths:
#
#   --stable   (default) Target x86_64-unknown-linux-gnu and link the
#              precompiled core/alloc from the sysroot. The `fullrust` crate
#              supplies the mem*/strlen intrinsics and unwind abort-stubs that
#              the precompiled, unwind-built `liballoc` expects. Works on stable
#              Rust. Larger binaries (they carry alloc's unwind tables, unused
#              under panic=abort).
#
#   --nightly  Target the custom freestanding triple `x86_64-fullrust-linux`
#              (targets/*.json) and recompile core/alloc/compiler_builtins from
#              source with `-Z build-std`. No unwinding is emitted, so binaries
#              are much smaller. Requires a nightly toolchain with rust-src.
#
# The two paths deliberately use DIFFERENT target triples. The flags below are
# injected via CARGO_TARGET_<TRIPLE>_* env vars keyed to the active triple, so
# they never touch host build scripts / proc-macros (which build for the host
# -gnu triple with the system linker).
#
# Usage:
#   ./x build [--nightly] [--release] [cargo args...]
#   ./x run --nightly hello
#   ./x run -p hello -- arg1 arg2
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

TOOLCHAIN=stable
ARGS=()       # cargo args, before any `--`
PROG=()       # program args, from `--` onward (for `./x run ... -- prog args`)
SAWDD=0
for a in "$@"; do
  if [ "$SAWDD" = 1 ]; then PROG+=("$a"); continue; fi
  case "$a" in
    --stable)  TOOLCHAIN=stable ;;
    --nightly) TOOLCHAIN=nightly ;;
    --)        SAWDD=1; PROG+=("$a") ;;   # keep the `--`; cargo flags go before it
    *)         ARGS+=("$a") ;;
  esac
done

if [ "${#ARGS[@]}" -eq 0 ]; then
  echo "usage: ./x <build|run|...> [--stable|--nightly] [cargo args]" >&2
  exit 2
fi

# Resolve the ld.lld shim shipped with the chosen toolchain. Its name (ld.lld)
# makes lld's generic driver select GNU/ELF mode; the bare `rust-lld` name does
# not.
SYSROOT="$(rustup run "$TOOLCHAIN" rustc --print sysroot)"
HOST="$(rustup run "$TOOLCHAIN" rustc -vV | sed -n 's/^host: //p')"
LLD="$SYSROOT/lib/rustlib/$HOST/bin/gcc-ld/ld.lld"
if [ ! -x "$LLD" ]; then
  echo "error: ld.lld not found at $LLD" >&2
  echo "       (toolchain may be missing the llvm-tools / rust-lld component)" >&2
  exit 1
fi

EXTRA=()
if [ "$TOOLCHAIN" = nightly ]; then
  TARGET="$HERE/targets/x86_64-fullrust-linux.json"
  ENVTRIPLE=X86_64_FULLRUST_LINUX
  # Most freestanding settings (relocation model, panic strategy, no unwind
  # tables) live in the target JSON, so only -static is needed here.
  FLAGS="-C link-args=-static"
  EXTRA=(-Z build-std=core,alloc,compiler_builtins -Z json-target-spec)
else
  TARGET=x86_64-unknown-linux-gnu
  ENVTRIPLE=X86_64_UNKNOWN_LINUX_GNU
  FLAGS="-C relocation-model=static -C linker-flavor=ld -C link-args=-static"
fi

export "CARGO_TARGET_${ENVTRIPLE}_LINKER=$LLD"
export "CARGO_TARGET_${ENVTRIPLE}_RUSTFLAGS=$FLAGS"

set -x
exec rustup run "$TOOLCHAIN" cargo "${ARGS[@]}" --target "$TARGET" "${EXTRA[@]}" "${PROG[@]}"
