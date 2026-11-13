#!/usr/bin/env bash
# Build and check the fullrust stack-overflow handler: main-thread and
# spawned-thread overflows must print "has overflowed its stack" and abort (134);
# a genuine wild write must pass through as an ordinary SIGSEGV (139), with no
# false overflow message.
set -u
cd "$(dirname "$0")"
V="${1:?usage: run.sh <minor, e.g. 1.100>}"
# With RUSTC (stage1 rustc) and CARGO (bootstrap cargo) exported, as run-tests.sh
# does, use those; otherwise the rustup-linked fullrust-<minor> toolchain.
if [ -n "${RUSTC:-}" ] && [ -n "${CARGO:-}" ]; then CARGO_CMD=("$CARGO"); else CARGO_CMD=(cargo "+fullrust-$V"); fi
rm -rf target
CARGO_TARGET_DIR=target "${CARGO_CMD[@]}" build --release --target x86_64-unknown-linux-fullrust >/dev/null 2>&1 || {
    echo "build failed"; exit 1; }
BIN=./target/x86_64-unknown-linux-fullrust/release/stackoverflow-fullrust
fail=0

check_overflow() {
    local mode="$1" name="$2"
    local out; out=$("$BIN" "$mode" 2>&1); local code=$?
    if [ "$code" = 134 ] && echo "$out" | grep -q "thread '$name' has overflowed its stack"; then
        echo "ok   $mode overflow -> message + abort"
    else
        echo "FAIL $mode overflow (exit=$code): $out"; fail=1
    fi
}

check_overflow main main
check_overflow thread overflower

out=$("$BIN" segv 2>&1); code=$?
if [ "$code" = 139 ] && ! echo "$out" | grep -q "overflowed"; then
    echo "ok   genuine SIGSEGV passes through (139, no false message)"
else
    echo "FAIL segv (exit=$code): $out"; fail=1
fi

if [ "$fail" = 0 ]; then echo "ALL STACK-OVERFLOW TESTS PASSED"; else echo "STACK-OVERFLOW TESTS FAILED"; exit 1; fi
