#!/usr/bin/env bash
# Build and run test crates against a freshly-built stage1 toolchain (no rustup
# link needed: stage1 rustc + the bootstrap cargo from the same build tree).
# Each test crate exits non-zero on failure; the binary must also be a static,
# libc-free ELF (no NEEDED entries).
#
# Usage:  ./run-tests.sh 1.95 [test-hello test-process ...]
set -euo pipefail

MINOR="${1:?usage: run-tests.sh <minor> [test crates...]}"; shift
HERE="$(cd "$(dirname "$0")" && pwd)"
BUILD="$HERE/rust-$MINOR/build/x86_64-unknown-linux-gnu"
TARGET="x86_64-unknown-linux-fullrust"
CRATES=("$@")
[[ ${#CRATES[@]} -gt 0 ]] || CRATES=(test-hello test-process test-syscall test-osextra)

export RUSTC="$BUILD/stage1/bin/rustc"
CARGO="$BUILD/stage0/bin/cargo"
export RUSTC_BOOTSTRAP=1   # some test crates use #![feature]
[[ -x "$RUSTC" ]] || { echo "missing $RUSTC — run build-fork.sh first" >&2; exit 1; }

failed=()
for crate in "${CRATES[@]}"; do
  echo "::group::$crate"
  dir="$HERE/$crate"
  bin="$(sed -n 's/^name = "\(.*\)"/\1/p' "$dir/Cargo.toml" | head -1)"
  ok=1
  "$CARGO" build --release --target "$TARGET" --manifest-path "$dir/Cargo.toml" || ok=0
  exe="$dir/target/$TARGET/release/$bin"
  if [[ $ok == 1 ]]; then
    if readelf -d "$exe" 2>/dev/null | grep -qi NEEDED; then
      echo "$crate: binary has NEEDED entries (not libc-free)"; ok=0
    fi
    (cd "$dir" && "$exe" one two three) || ok=0
  fi
  echo "::endgroup::"
  if [[ $ok == 1 ]]; then echo "PASS $crate"; else echo "::error::FAIL $crate"; failed+=("$crate"); fi
done

if [[ ${#failed[@]} -gt 0 ]]; then
  echo "failed: ${failed[*]}"; exit 1
fi
echo "all ${#CRATES[@]} test crates passed"
