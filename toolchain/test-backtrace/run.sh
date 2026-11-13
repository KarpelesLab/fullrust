#!/usr/bin/env bash
# Build (with debuginfo) and check fullrust panic backtraces: frame-pointer
# unwinding + gimli/DWARF symbolization reading /proc/self/exe.
set -u
cd "$(dirname "$0")"
V="${1:?usage: run.sh <minor, e.g. 1.100>}"
# With RUSTC (stage1 rustc) and CARGO (bootstrap cargo) exported, as run-tests.sh
# does, use those; otherwise the rustup-linked fullrust-<minor> toolchain.
if [ -n "${RUSTC:-}" ] && [ -n "${CARGO:-}" ]; then CARGO_CMD=("$CARGO"); else CARGO_CMD=(cargo "+fullrust-$V"); fi
rm -rf target
CARGO_TARGET_DIR=target "${CARGO_CMD[@]}" build --release --target x86_64-unknown-linux-fullrust >/dev/null 2>&1 || {
    echo "build failed"; exit 1; }
BIN=./target/x86_64-unknown-linux-fullrust/release/backtrace-fullrust
fail=0
pass() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fail=1; }

# 1. Direct capture API resolves names + file:line.
cap=$("$BIN" capture 2>&1)
echo "$cap" | grep -q "status=Captured" && pass "Backtrace::status() == Captured" || bad "status not Captured"
echo "$cap" | grep -q "backtrace_fullrust::main" && pass "capture resolves symbol names" || bad "no symbol names"
echo "$cap" | grep -q "src/main.rs:" && pass "capture resolves file:line" || bad "no file:line"

# 2. Auto panic backtrace with RUST_BACKTRACE=1 shows the call chain.
pb=$(RUST_BACKTRACE=1 "$BIN" panic 2>&1)
for f in deep_c deep_b deep_a; do
    echo "$pb" | grep -q "backtrace_fullrust::$f" || bad "panic backtrace missing frame $f"
done
echo "$pb" | grep -q "backtrace_fullrust::deep_a" && pass "panic backtrace shows the call chain" || true
echo "$pb" | grep -qE "deep_c$|deep_c\b" && echo "$pb" | grep -q "src/main.rs:12" && pass "panic backtrace has file:line" || bad "panic backtrace missing file:line"

# 3. Without RUST_BACKTRACE, the note is shown and no frames printed.
nb=$("$BIN" panic 2>&1)
echo "$nb" | grep -q "RUST_BACKTRACE=1" && ! echo "$nb" | grep -qE "^\s+[0-9]+: backtrace_fullrust" \
    && pass "no backtrace unless RUST_BACKTRACE set" || bad "unexpected backtrace/note behavior"

if [ "$fail" = 0 ]; then echo "ALL BACKTRACE TESTS PASSED"; else echo "BACKTRACE TESTS FAILED"; exit 1; fi
