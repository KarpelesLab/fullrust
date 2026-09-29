#!/usr/bin/env python3
"""Mechanically teach a crate's cfg predicates about target_os = "fullrust".

fullrust is ABI-identical to x86_64 Linux but is neither `unix` nor
`target_os = "linux"`.  This rewrites every *predicate* in `#[cfg(..)]`,
`#![cfg(..)]`, `cfg!(..)`, the predicate half of `#[cfg_attr(PRED, ..)]` and
tokio's `feature! { #![PRED] .. }`
(doc(cfg(..)) and comments are left alone) so that fullrust takes the
Linux/glibc branch everywhere:

    unix                  -> any(unix, target_os = "fullrust")
    target_os = "linux"   -> any(target_os = "linux", target_os = "fullrust")
    target_env = "gnu"    -> any(target_env = "gnu", target_os = "fullrust")

Usage: fullrust-cfg.py [--keep WORD]... FILE...   (rewrites in place, idempotent)

--keep WORD leaves every `all(..)` group that mentions WORD untouched (e.g.
`--keep tokio_unstable`: unstable-only Linux features whose Linux-only
dependencies a fullrust build does not have stay off).
"""
import re
import sys

FR = 'target_os = "fullrust"'
LEAVES = [
    (re.compile(r'(?<!["\w])unix(?!["\w])'), f'any(unix, {FR})'),
    (re.compile(r'target_os\s*=\s*"linux"'), f'any(target_os = "linux", {FR})'),
    (re.compile(r'target_env\s*=\s*"gnu"'), f'any(target_env = "gnu", {FR})'),
]
KEEP = []
# `feature! { #![PRED] .. }` is tokio's cfg-wrapping macro.
START = re.compile(r'#!?\[\s*cfg\s*\(|cfg!\s*\(|#!?\[\s*cfg_attr\s*\(|feature!\s*\{\s*#!\[')


def mask_kept(p):
    """Replace each innermost balanced `all(..)` group that mentions a --keep
    word by a placeholder; returns (masked text, saved groups)."""
    saved = []
    if not KEEP:
        return p, saved
    spans = []
    for m in re.finditer(r'\ball\s*\(', p):
        depth, k = 1, m.end()
        while k < len(p) and depth:
            depth += {'(': 1, ')': -1}.get(p[k], 0)
            k += 1
        if any(w in p[m.start():k] for w in KEEP):
            spans.append((m.start(), k))
    # keep only innermost matching groups
    spans = [a for a in spans
             if not any(b != a and a[0] <= b[0] and b[1] <= a[1] for b in spans)]
    out, i = [], 0
    for a, b in spans:
        out.append(p[i:a])
        out.append('\0%d\0' % len(saved))
        saved.append(p[a:b])
        i = b
    out.append(p[i:])
    return ''.join(out), saved


def rewrite_pred(p):
    if 'fullrust' in p:  # already processed
        return p
    out, saved = mask_kept(p)
    for rx, rep in LEAVES:
        out = rx.sub(rep, out)
    for n, grp in enumerate(saved):
        out = out.replace('\0%d\0' % n, grp)
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
            if c in '([':
                depth += 1
            elif c in ')]':
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


args = sys.argv[1:]
while args and args[0] == '--keep':
    KEEP.append(args[1])
    args = args[2:]
for f in args:
    s = open(f).read()
    n = process(s)
    if n != s:
        open(f, 'w').write(n)
        print('rewrote', f)
