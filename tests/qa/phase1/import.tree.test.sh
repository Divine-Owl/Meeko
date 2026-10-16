#!/usr/bin/env bash
# tests/qa/phase1/import.tree.test.sh
# Phase 1 gate: the imported quickshell/hypr/system trees must be byte-identical
# to refs/impasto (the pinned upstream input). No additions, no omissions, no
# edits — this is the verbatim import that CR&S pass 1 audits as "mechanical only."
#
# UPSTREAM_REFS may override the upstream input directory — used when this script
# runs inside a detached worktree, whose gitignored refs/ tree is absent
# (see CR&S's pass-2 method). Unset it to use the default path. A missing or
# non-directory path fails loudly rather than silently degrading the check.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
UPSTREAM="${UPSTREAM_REFS:-$REPO_ROOT/refs/impasto}"
if [ ! -d "$UPSTREAM" ]; then
    echo "import.tree: UPSTREAM_REFS is not a directory: $UPSTREAM" >&2
    exit 2
fi

fail=0

check_tree() {
    local upstream_dir="$1"   # e.g. home/.config/quickshell (relative to each root)
    local import_dir="$2"     # e.g. home/.config/quickshell (relative to repo root)
    local name="$3"

    while IFS= read -r rel; do
        # skip the .git dirs inside the reference clone
        case "$rel" in .git*) continue ;; esac
        local src="$UPSTREAM/$upstream_dir/$rel"
        local dst="$REPO_ROOT/$import_dir/$rel"
        if [ ! -f "$dst" ]; then
            echo "MISSING ($name): $import_dir/$rel"
            fail=1
        elif ! cmp -s "$src" "$dst"; then
            echo "DIFFERS ($name): $import_dir/$rel"
            fail=1
        fi
    done < <(cd "$UPSTREAM/$upstream_dir" && find . -type f -printf '%P\n' | sort)

    # No extra files in the import that aren't in upstream (excluding .git)
    while IFS= read -r rel; do
        case "$rel" in .git*) continue ;; esac
        local src="$UPSTREAM/$upstream_dir/$rel"
        local dst="$REPO_ROOT/$import_dir/$rel"
        if [ ! -f "$src" ]; then
            echo "EXTRA ($name): $import_dir/$rel (not in upstream)"
            fail=1
        fi
    done < <(cd "$REPO_ROOT/$import_dir" && find . -type f -not -path './.git/*' -printf '%P\n' | sort)
}

check_tree home/.config/quickshell home/.config/quickshell quickshell
check_tree home/.config/hypr home/.config/hypr hypr
check_tree system system system

if [ "$fail" -eq 0 ]; then
    echo "import.tree: PASS — quickshell, hypr, system trees byte-identical to refs/impasto"
    exit 0
else
    echo "import.tree: FAIL — tree is not a verbatim import"
    exit 1
fi
