# Meeko — Code Review & Security audit preconditions (Phase 0)

Date: 2026-10-15 · Owner: Code Review & Security · Derived from
`docs/superpowers/specs/2026-10-15-meeko-shell-design.md` (working-tree
revision incl. swap-policy, session-env, and `ci/check.sh` amendments).

These are the conditions under which I will return **APPROVED**. Anything
listed here that lands in a diff without its precondition met gets
**CHANGES_REQUESTED** with the spec section cited. Review happens per phase
gate, not at the end.

## 1. Standing rules (all phases)

- **Threat model honesty** (§12): same-uid trusted. The `/proc/<pid>/exe` role
  check is defense-in-depth, never described in code comments, docs, or commit
  messages as authentication. Any new doc that blurs this gets corrected.
- **License hygiene** (§3): GPL-3.0 headers on new/ported files; no Coucou
  name, Mochi, or sounds anywhere in the tree; `refs/` never committed
  (gitignored vendor inputs). CI license/attribution check must exist before
  Phase 1 merges (DevOps).
- **No secrets anywhere**: no tokens in code, tests, fixtures, logs, env
  defaults, or CI. Dummy token in fixtures only. gitleaks gate green on every
  merge commit.
- **Contracts are law** (§6): every verb and pushed event has JSON success
  **and** error shapes mirroring impasto's `{"ok":…}` / `{"error":…}`. A new
  verb without both shapes in the same diff is a reject, not a follow-up.
- **Diff scope**: Phase 1 is a mechanical rename/import — any behavioral or
  visual change smuggled into it is a reject (pixel-compatibility, §7).

## 2. Per-phase audit gates

### Phase 1 — skeleton (MEEKO-003)
- Module rename `impasto` → `meeko` is mechanical; `git diff --stat` shows no
  logic churn in inherited services beyond identifier renames.
- Superseded modules actually removed (CommandCode/OpenRouter/Claude/Codex/
  ImpastoMemory pollers), Games/Pet cut; removed Python pollers leave no
  orphan autostart/cron references.
- Inherited `qs ipc` verbs unchanged in behavior; `meeko status`/`meeko panel`
  added per §6.2, no verdict verbs.

### Phase 2 — hookd (the security boundary; strictest gate)
Must all be true in code, each traceable to spec §6/§10/§12:
1. **Token module** (§6.3): `~/.codync/token` read with owner-uid check,
   `mode & 0o077 == 0`, `O_NOFOLLOW`; degraded read-only start on failure with
   reason logged and contents never logged; re-read on 401/mtime change; token
   unreachable from the socket module by construction (not convention).
2. **Socket module**: `0600` sock in `0700` dir; `SO_PEERCRED` uid check
   before any parse; ≤8 connections with idle/dead-client drop; 1 MiB hard
   line cap plus per-verb caps (≤1 KiB control, ≤8 KiB `send.text`) enforced
   pre-parse; unknown verb ⇒ `ok:false`, daemon survives.
3. **Verb gate**: `respondApproval`/`send`/`stop` validate against the local
   projection (pending/known) and reject stale ids locally — I will grep the
   mock-host call log for "nothing proxied" assertions (QA's B-class cases).
4. **Host client**: URL pinned loopback; non-loopback only behind `MEEKO_TEST=1`
   and hard startup error otherwise; jittered backoff 1s→30s; rev gap > 5000 ⇒
   full resync; no host-only data (entry bodies, credentials, teams) cached.
5. **Projection**: memory-only, all five bounds enforced with FIFO drop +
   `dropCount` in snapshot; no `unsafe`, no `unwrap()` on parse paths of
   untrusted input (socket lines, SSE lines) — malformed input returns an
   error envelope or drops, never panics.
6. **Logging**: structured + redacted — no chat content, no tokens, no peer
   payloads in log lines at any level.

### Phase 3 — agent surface
- Approval card verdicts flow only through `MeekoService` socket, never
  `qs ipc` (§6.2); QML holds no token and never constructs host URLs.
- Push-driven only: no `Timer` polling of `MeekoService` in any new QML file
  (grep gate). Repaint throttling on streaming `turn.add` present.
- `preview` rendering is display-escaped (no `HTML`/`RichText` on agent-supplied
  strings; ≤120 chars enforced at projection, not in QML).

### Phase 4 — efficiency + units
- systemd units: `MemoryHigh`/`MemoryMax` exactly 350M/430M + 64M (§10),
  `Restart=on-failure` with 1s delay, `graphical-session.target` binding,
  session-env import present, no `Requires=` edge from quickshell → hookd.
- Quiet mode: triggers push-only; repaint counter monotonic; no catch-up burst
  on exit (QA test cited).
- `setup`: swap step skippable + detects existing swap (never overwrites) +
  dry-run; root via pkexec only; env-file generation mode `0600`.

## 3. CI gate requirements (DevOps, before first merge)

- Actions pinned by commit SHA, no `pull_request_target` + PR-head checkout.
- Jobs: qmllint · syntax (setup check) · hookd tests vs QA mock · gitleaks ·
  license/attribution check · `cargo audit` · ESLint-security on any JS ·
  bandit on remaining Python. `ci/check.sh` is the offline mirror of exactly
  this set.
- Fixture sign-off: every committed fixture under `tests/fixtures/` gets my
  scrub review (synthetic or content-scrubbed) before it lands — QA already
  committed to citing fixture id + rev sequence; I sign off per fixture file.

## 4. Two open items I am tracking (not blocking Phase 0)

1. **Budget number conflict**: PRD §4 says idle ≤250 MB; spec §2/§10 says
   ≤500 MB total + 1.25× baseline. @Architect: record the supersession
   explicitly in the PRD so audit and QA assert one number.
2. **`qs ipc` exposure**: §6.2's `meeko status` returns agent/approval data to
  any same-uid caller — fine under the threat model, but the status payload
  should stay summary-level (no turn previews) unless a use requires it;
  re-check at Phase 3 review.

Verdicts will be posted in-room per phase gate: APPROVED / CHANGES_REQUESTED
with line-specific fixes.
