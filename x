#!/usr/bin/env bash
# fullrust build wrapper.
#
# Wires up the Rust-bundled LLVM linker (rust-lld, invoked as `ld.lld`) for the
# active toolchain and selects one of two libc-free build strategies:
#
#   --stable   (default) Link the precompiled core/alloc from the sysroot. The
#              `fullrust` crate supplies the mem* intrinsics and unwind abort
#              stubs that the precompiled, unwind-built `liballoc` expects.
#              Works on stable Rust. Larger binaries (carries alloc's unwind
#              tables, which are unused under panic=abort).
#
#   --nightly  Recompile core/alloc/compiler_builtins from source with
#              `-Z build-std` and `panic_immediate_abort`, so no unwinding code
#              is ever emitted. Much smaller binaries. Requires a nightly
#              toolchain with the rust-src component.
#
# Usage:
#   ./x build [--nightly] [cargo args...]
#   ./x run --nightly hello
#   ./x run -p hello
#
set -euo pipefail

TOOLCHAIN=stable
BUILD_STD=()
ARGS=()
for a in "$@"; do
  case "$a" in
    --stable)  TOOLCHAIN=stable ;;
    --nightly) TOOLCHAIN=nightly ;;
    *)         ARGS+=("$a") ;;
  esac
done

if [ "${#ARGS[@]}" -eq 0 ]; then
  echo "usage: ./x <build|run|...> [--stable|--nightly] [cargo args]" >&2
  exit 2
fi

# Resolve the ld.lld shim shipped with the chosen toolchain. Its name (ld.lld)
# makes lld's generic driver pick GNU/ELF mode; the bare `rust-lld` name does not.
SYSROOT="$(rustup run "$TOOLCHAIN" rustc --print sysroot)"
HOST="$(rustup run "$TOOLCHAIN" rustc -vV | sed -n 's/^host: //p')"
LLD="$SYSROOT/lib/rustlib/$HOST/bin/gcc-ld/ld.lld"
if [ ! -x "$LLD" ]; then
  echo "error: ld.lld not found at $LLD" >&2
  echo "       (the toolchain may be missing the llvm-tools / rust-lld component)" >&2
  exit 1
fi
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$LLD"

if [ "$TOOLCHAIN" = nightly ]; then
  # Recompile the sysroot crates without unwinding. We deliberately do NOT
  # enable `compiler-builtins-mem`: the fullrust crate provides the mem*
  # intrinsics on both paths, so there is a single source of truth.
  BUILD_STD=(
    -Z build-std=core,alloc,compiler_builtins
    -Z build-std-features=panic_immediate_abort
  )
fi

set -x
exec rustup run "$TOOLCHAIN" cargo "${ARGS[@]}" "${BUILD_STD[@]}"
