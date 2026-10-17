#!/usr/bin/env bash
# ci/check.sh — the one gate for Meeko.
#
# The GitHub Actions workflow runs this exact script (with MEEKO_CI=1), and
# running it by hand is the local mirror: same checks, same order, same exit
# code. What differs is strictness — in CI a missing tool is a failure; on a
# dev box it is a loud skip, so a laptop without gitleaks still gets useful
# feedback instead of a red herring.
#
# Usage:
#   ci/check.sh                  full gate (everything applicable on this box)
#   ci/check.sh --fast           pre-commit subset: syntax + contract, seconds
#   ci/check.sh --import-check    Phase 1 verbatim-import checkpoint. Only
#                                meaningful against commit 71af2ed's tree: the
#                                rename (0b818d9) and deletion (4bbc24b)
#                                commits differ from refs/impasto BY DESIGN, so
#                                this is not part of the standing gate — audit
#                                runs it in a detached worktree at 71af2ed.
#                                Detached worktrees lack the gitignored refs/,
#                                so run it as:
#                                  UPSTREAM_REFS=<clone>/refs/impasto ci/check.sh --import-check
#                                The env var is the impasto dir itself, not the
#                                refs root (the test's own header says so too).
#   ci/check.sh --help
#
# MEEKO_CI=1 (set by the workflow) turns every "skip" into a failure, so a CI
# run that says PASS can never have silently skipped a gate.
set -uo pipefail          # deliberately no -e: failures accumulate and report together

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

STRICT="${MEEKO_CI:-0}"
FAST=0
IMPORT=0

for arg in "$@"; do
    case "$arg" in
        --fast)        FAST=1 ;;
        --import-check) IMPORT=1 ;;
        -h|--help)
            sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "ci/check.sh: unknown argument: $arg (try --help)" >&2
            exit 2
            ;;
    esac
done

fails=0
skips=0

section() { printf '\n── %s ──\n' "$1"; }
pass()    { printf '   ok   %s\n' "$1"; }
failure() { printf '   FAIL %s\n' "$1"; fails=$((fails + 1)); }

# skip <what> <tool> <why> — fatal under MEEKO_CI=1, a note otherwise.
skip() {
    if [ "$STRICT" = 1 ]; then
        failure "$1 — required in CI but not available ($2)"
    else
        printf '   skip %s (no %s — %s)\n' "$1" "$2" "$3"
        skips=$((skips + 1))
    fi
}

# run_or_fail <label> <cmd...> — stdout/stderr indented under the result.
run_or_fail() {
    local label="$1" out
    shift
    if out=$("$@" 2>&1); then
        pass "$label"
    else
        failure "$label"
        printf '%s\n' "$out" | sed 's/^/        /'
    fi
}

# ── 1. shell syntax ─────────────────────────────────────────────────────
# setup check covers home/ (py/lua/zsh/json/toml + `bash -n setup`); this
# adds every tracked *.sh — the test suite, this script, hooks, future CI.
section "shell syntax (bash -n)"
sh_fail=0
while IFS= read -r f; do
    [ -f "$f" ] || continue
    if ! out=$(bash -n "$f" 2>&1); then
        printf '   FAIL bash -n %s\n%s\n' "$f" "$out" | sed 's/^/        /'
        sh_fail=1
    fi
done < <({ git ls-files '*.sh' '.githooks/*'; echo 'ci/check.sh'; echo '.githooks/pre-commit'; } | sort -u)
if ! bash -n setup 2>&1; then sh_fail=1; printf '   FAIL bash -n setup\n'; fi
if [ "$sh_fail" -eq 0 ]; then pass "all tracked shell scripts parse"; else fails=$((fails + 1)); fi

# ── 2. tree syntax (upstream's own aggregate check) ─────────────────────
section "syntax (setup check: python lua zsh bash json toml)"
run_or_fail "setup check" ./setup check

# ── 3. contract: the qs ipc verb baseline ───────────────────────────────
# Upstream-derived (refs/impasto), so it can't pass vacuously — CR&S pass 3.
section "contract (qs ipc verb baseline)"
run_or_fail "verbs.static" bash tests/qa/phase1/verbs.static.test.sh

# ── 4. Phase 1 import checkpoint (opt-in only) ──────────────────────────
if [ "$IMPORT" -eq 1 ]; then
    section "checkpoint (verbatim import — commit-scoped, see header)"
    run_or_fail "import.tree" bash tests/qa/phase1/import.tree.test.sh
fi

# --fast stops here: everything below is pre-commit-unfriendly (qmllint,
# gitleaks, cargo) and belongs to the full gate / CI.
if [ "$FAST" -eq 1 ]; then
    section "summary"
    if [ "$fails" -gt 0 ]; then
        printf 'ci/check --fast: FAIL — %d failed, %d skipped\n' "$fails" "$skips"
        exit 1
    fi
    printf 'ci/check --fast: PASS (%d skipped)\n' "$skips"
    exit 0
fi

# ── 5. qmllint ──────────────────────────────────────────────────────────
# The linter is pinned by path, not taken from PATH: on most boxes the bare
# `qmllint` is Qt5's (qt5-declarative) and exits 255 with *no output* on any
# Quickshell file — which used to read as "lint is broken here" and let this
# gate pass while linting nothing. Warnings are ratcheted by a committed
# baseline so legacy noise can't hide new breakage; errors always fail.
section "qml lint (qmllint — Qt6, errors fail, warnings ≤ baseline)"
if command -v node >/dev/null 2>&1; then
    if out=$(node ci/qmllint-gate.mjs 2>&1); then
        pass "qmllint (Qt6 gate)"
    else
        failure "qmllint (Qt6 gate)"
        printf '%s\n' "$out" | sed 's/^/        /'
    fi
else
    skip "qmllint" "node" "package: nodejs"
fi

# ── 6. secret scan ──────────────────────────────────────────────────────
# CR&S requirement. Fixtures under tests/fixtures/ are covered too — they are
# scrubbed before landing, this catches what slips through.
section "secret scan (gitleaks)"
if command -v gitleaks >/dev/null 2>&1; then
    # `gitleaks dir` scans the working tree (v8.19+); fall back to the older
    # `detect --source` spelling for pinned/older installs.
    if gitleaks dir --help >/dev/null 2>&1; then
        run_or_fail "gitleaks (tree)" gitleaks dir "$REPO_ROOT" --redact --no-banner
    else
        run_or_fail "gitleaks (tree)" gitleaks detect --source "$REPO_ROOT" --redact --no-banner
    fi
else
    skip "gitleaks" "gitleaks" "cargo install gitleaks"
fi

# ── 7. cargo (Phase 2+: meeko-hookd) ────────────────────────────────────
# Present only once a Cargo.toml lands in the tree; then fmt/test become part
# of the standing gate. Full release builds stay in CI (memory-tight box).
section "rust (cargo)"
cargo_manifest=$(git ls-files 'Cargo.toml' '**/Cargo.toml' | head -1 || true)
if [ -z "$cargo_manifest" ]; then
    # Not applicable yet — never a skip: CI must stay green through Phase 1.
    printf '   n/a  no Cargo.toml in tree yet (Phase 2+)\n'
elif ! command -v cargo >/dev/null 2>&1; then
    skip "cargo fmt+test" "cargo" "package: rust"
else
    if command -v rustfmt >/dev/null 2>&1 || cargo fmt --version >/dev/null 2>&1; then
        run_or_fail "cargo fmt --check" cargo fmt --manifest-path "$cargo_manifest" -- --check
    else
        skip "cargo fmt" "rustfmt" "package: rust"
    fi
    # --locked: CI must test the exact Cargo.lock that `setup` installs from,
    # or a daemon built at install time can resolve newer crates than the
    # ones under test here and the gate proves nothing about it.
    run_or_fail "cargo test" cargo test --locked --manifest-path "$cargo_manifest"
fi

# ── 8. contract tests (hookd vs mock host) ───────────────────────────────
# Phase 2+: real hookd binary + mock Codync host + real unix socket.
# Covers HOOKD-01..15, GATE-01..05, HOST-02/03, FLOW-01..03, PROJ-01..04, SEC-04.
section "contract (hookd vs mock host)"
if ! command -v node >/dev/null 2>&1; then
    skip "contract" "node" "package: nodejs"
elif [ -z "$cargo_manifest" ]; then
    printf '   n/a  no Cargo.toml in tree yet (Phase 2+)\n'
else
    # Build unconditionally. The suite's own before() skips cargo when the
    # binary merely *exists*, so a stale meeko-hookd (older checkout, shared
    # target dir) silently runs against the current test file and turns real
    # contract drift into "the tests are flaky". --locked for the same reason
    # as the tests above: this is the lockfile the install path uses.
    run_or_fail "cargo build (hookd)" cargo build --locked --manifest-path "$cargo_manifest"
    # git stores no mode bits beyond the exec bit, so a fresh clone lands this
    # fixture at 0644 — hookd refuses group/other-readable tokens and the whole
    # suite dies on "socket not found". Set it here, once, for every clone.
    if [ -f tests/fixtures/token ]; then
        chmod 600 tests/fixtures/token
        pass "fixture token mode 0600"
    else
        failure "fixture token missing (tests/fixtures/token)"
    fi
    run_or_fail "contract" node --test tests/qa/phase2/contract.test.js
fi

# ── 8b. phase 3 agent-surface suite (vitest) ─────────────────────────────
# QA's Phase 3 file is vitest-based, not node:test — it needs vitest on the
# box (CI installs it; a dev box may not).
section "agent-surface (vitest)"
if [ ! -f tests/qa/phase3/agent-surface.test.js ]; then
    printf '   n/a  no phase 3 suite in tree\n'
elif ! command -v node >/dev/null 2>&1; then
    skip "agent-surface" "node" "package: nodejs"
elif ! npx --no-install vitest --version >/dev/null 2>&1; then
    skip "agent-surface" "vitest" "npm install --save-dev vitest"
else
    run_or_fail "agent-surface" npx --no-install vitest run tests/qa/phase3
fi

# ── 9. whitespace hygiene ───────────────────────────────────────────────
section "whitespace (git diff --check)"
if out=$(git diff --check 2>&1) && [ -z "$out" ]; then
    pass "no whitespace errors in working tree"
else
    failure "git diff --check"
    printf '%s\n' "$out" | sed 's/^/        /'
fi

# ── 10. systemd user units ───────────────────────────────────────────────
# Phase 4 ships two units. `systemd-analyze verify` exits 0 on a bad directive
# ("Unknown key … ignoring") and non-zero when ExecStart isn't installed — the
# opposite of what you'd expect, and binaries are legitimately absent in CI —
# so the verdict is read from the output, not the exit code.
section "systemd (user units)"

# A unit ExecStarting a /usr/local binary is ours to install: nothing else on
# the box provides it. Phase 4 shipped exactly that — meeko-hookd.service with
# no producer anywhere in setup — so systemd started a daemon that wasn't
# there, 203/EXEC, Restart=on-failure, and the log reads as a broken daemon
# rather than a step the install never did. Static text, so it runs without
# systemd-analyze too: this is the check that would have caught the original
# gap in CI, not one that depends on the host.
while IFS= read -r u; do
    while IFS= read -r target; do
        case $target in
            /usr/local/*)
                if grep -qF -- "$target" setup && grep -qE 'cargo (build|install)' setup; then
                    pass "producer in setup: $target"
                else
                    failure "no producer in setup for $target (unit would 203/EXEC)"
                fi
                ;;
        esac
    done < <(sed -n 's/^ExecStart=[[:space:]]*//p' "$u" | awk '{print $1}')
done < <(git ls-files 'system/**/*.service')

if command -v systemd-analyze >/dev/null 2>&1; then
    unit_bad=0
    while IFS= read -r u; do
        out=$(systemd-analyze --user verify "$u" 2>&1) || true
        bad=$(printf '%s\n' "$out" | grep -E "Unknown (key|section)|Invalid |Duplicate |Failed to (parse|resolve)" || true)
        if [ -n "$bad" ]; then
            printf '   FAIL systemd verify %s\n' "$u"
            printf '%s\n' "$bad" | sed 's/^/        /'
            unit_bad=1
        else
            pass "systemd verify $u"
        fi
    done < <(git ls-files 'system/**/*.service')
    if [ "$unit_bad" -ne 0 ]; then fails=$((fails + 1)); fi
else
    skip "systemd-analyze" "systemd-analyze" "package: systemd"
fi

# ── 11. setup (dry-run intent) ──────────────────────────────────────────────
# What the installer *says* it will do, asserted — the class of bug this phase
# kept producing is a step that exists but never fires, and a report that
# never mentions it is how that hides. Staged into a fresh prefix so the
# verdict does not depend on what this box already installed: deploy reports
# "new" for every file in an empty prefix. Dry-run is read-only by
# construction (every change funnels through act); on a live box it was
# checked by diffing system state before and after two runs.
section "setup (dry-run intent)"
stage=$(mktemp -d)
if out=$(IMPASTO_ROOT="$stage" timeout 120 ./setup system -n --noconfirm 2>&1); then
    for u in quickshell.service meeko-hookd.service; do
        if printf '%s\n' "$out" | grep -qF "/etc/systemd/user/$u"; then
            pass "setup reports: /etc/systemd/user/$u"
        else
            failure "setup dry-run never mentions /etc/systemd/user/$u"
        fi
    done
    if printf '%s\n' "$out" | grep -q "would build hookd"; then
        pass "setup reports: build+install /usr/local/bin/meeko-hookd"
    else
        failure "setup dry-run has no build/install step for meeko-hookd"
    fi
else
    failure "setup system --dry-run exited non-zero"
fi
rm -rf "$stage"

# ── summary ─────────────────────────────────────────────────────────────
section "summary"
if [ "$fails" -gt 0 ]; then
    printf 'ci/check: FAIL — %d failed, %d skipped\n' "$fails" "$skips"
    exit 1
fi
if [ "$STRICT" = 1 ] && [ "$skips" -gt 0 ]; then
    # Defensive: strict mode should have turned skips into failures already.
    printf 'ci/check: FAIL — %d skipped under MEEKO_CI=1\n' "$skips"
    exit 1
fi
printf 'ci/check: PASS (%d skipped)\n' "$skips"
exit 0
