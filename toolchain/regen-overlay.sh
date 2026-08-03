#!/usr/bin/env bash
# Regenerate patches/fullrust-<minor>.patch from the current rust-<minor> tree.
#
# The overlay must capture BOTH the main-repo edits and the edits inside the
# vendored submodules we patch (library/backtrace, library/stdarch). A
# superproject `git diff` only sees submodule *pointers*, never their file
# contents, so each submodule is diffed inside itself and path-prefixed back to
# the superproject root. New (untracked) files are included via `git add -N`.
#
# Usage:  ./regen-overlay.sh 1.88
set -euo pipefail

MINOR="${1:?usage: regen-overlay.sh <minor, e.g. 1.88>}"
HERE="$(cd "$(dirname "$0")" && pwd)"
RUST="$HERE/rust-$MINOR"
PATCH="$HERE/patches/fullrust-$MINOR.patch"

[[ -d "$RUST/.git" ]] || { echo "missing checkout $RUST" >&2; exit 1; }

# Vendored submodules the overlay may edit — but ONLY those this version actually
# registers as submodules. stdarch was a submodule through 1.89 but de-vendored
# into the main tree in 1.90 (std_detect moved to library/std_detect); on 1.90+
# its edits ride the main-repo diff (step 1), not a per-submodule diff (step 2).
# Keep this candidate list in sync with build-fork.sh.
SUBMODULES=()
for sm in library/backtrace library/stdarch; do
  if grep -qE "^[[:space:]]*path = $sm[[:space:]]*\$" "$RUST/.gitmodules" 2>/dev/null; then
    SUBMODULES+=("$sm")
  fi
done

cd "$RUST"

# 1. Main repo: every tracked edit + new file, EXCLUDING the patched submodules
#    (their gitlinks are unchanged; their contents come from step 2).
excludes=()
for sm in "${SUBMODULES[@]}"; do excludes+=(":(exclude)$sm"); done
git add -A
git diff --cached HEAD -- . "${excludes[@]}" > "$PATCH"
git reset -q

# 2. Each patched submodule: tracked edits + new files, path-prefixed so the
#    combined patch applies with `git apply` from the superproject root.
for sm in "${SUBMODULES[@]}"; do
  [[ -e "$sm/.git" ]] || { echo "submodule $sm not checked out" >&2; exit 1; }
  ( cd "$sm"
    git add -N .
    git diff HEAD --src-prefix="a/$sm/" --dst-prefix="b/$sm/" >> "$PATCH"
    git reset -q )
done

echo "regenerated $PATCH ($(wc -l < "$PATCH") lines)"
