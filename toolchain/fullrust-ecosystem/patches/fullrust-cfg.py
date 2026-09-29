#!/usr/bin/env python3
"""Mechanically teach a crate's cfg predicates about target_os = "fullrust".

fullrust is ABI-identical to x86_64 Linux but is neither `unix` nor
`target_os = "linux"`.  This rewrites every *predicate* in `#[cfg(..)]`,
`#![cfg(..)]`, `cfg!(..)` and the predicate half of `#[cfg_attr(PRED, ..)]`
(doc(cfg(..)) and comments are left alone) so that fullrust takes the
Linux/glibc branch everywhere:

    unix                  -> any(unix, target_os = "fullrust")
    target_os = "linux"   -> any(target_os = "linux", target_os = "fullrust")
    target_env = "gnu"    -> any(target_env = "gnu", target_os = "fullrust")

Usage: fullrust_cfg.py FILE...   (rewrites in place, idempotent)
"""
import re
import sys

FR = 'target_os = "fullrust"'
LEAVES = [
    (re.compile(r'(?<!["\w])unix(?!["\w])'), f'any(unix, {FR})'),
    (re.compile(r'target_os\s*=\s*"linux"'), f'any(target_os = "linux", {FR})'),
    (re.compile(r'target_env\s*=\s*"gnu"'), f'any(target_env = "gnu", {FR})'),
]
START = re.compile(r'#!?\[\s*cfg\s*\(|cfg!\s*\(|#!?\[\s*cfg_attr\s*\(')


def rewrite_pred(p):
    if 'fullrust' in p:  # already processed
        return p
    out = p
    for rx, rep in LEAVES:
        out = rx.sub(rep, out)
    return out


def process(src):
    out = []
    i = 0
    while True:
        m = START.search(src, i)
        if not m:
            out.append(src[i:])
            break
        line_start = src.rfind('\n', 0, m.start()) + 1
        if src[line_start:m.start()].lstrip().startswith('//'):
            out.append(src[i:m.end()])
            i = m.end()
            continue
        is_attr = 'cfg_attr' in m.group(0)
        j = m.end()
        depth = 1
        k = j
        while k < len(src):
            c = src[k]
            if c == '(':
                depth += 1
            elif c == ')':
                depth -= 1
                if depth == 0:
                    break
            elif c == ',' and depth == 1 and is_attr:
                break
            elif c == '"':
                k = src.index('"', k + 1)
            k += 1
        out.append(src[i:j])
        out.append(rewrite_pred(src[j:k]))
        i = k
    return ''.join(out)


for f in sys.argv[1:]:
    s = open(f).read()
    n = process(s)
    if n != s:
        open(f, 'w').write(n)
        print('rewrote', f)
