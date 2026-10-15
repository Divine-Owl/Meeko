# Meeko — QA Test Matrix (Phase 0)

Date: 2026-10-15 · Status: Phase 0 planning artifact — no test code exists yet ·
Owner: QA & Testing · Sources: `docs/superpowers/specs/2026-10-15-meeko-shell-design.md`
(authoritative; "spec §" refs below), `docs/PRD-draft-v0.md`.

Phase 0 output only: this matrix is the checklist QA will turn into suites during
Phases 1–4. Nothing here is executed yet.

## 0. How to read

- **Layers**: U = unit (hermetic, no display) · C = contract (real hookd + mock
  Codync host + real socket, no compositor) · E = end-to-end (headless Hyprland) ·
  P = budget/perf.
- **Gate**: BLOCKING = pass/fail gate for the referenced ticket DoD ·
  ADVISORY = measured and reported, non-gating.
- **IDs**: area prefix `HOOKD/HOST/PROJ/FLOW/IPC/QUIET/LAZY/BUDGET/FAIL/SEC/SETUP/SKEL`
  + number. Test reports and bug IDs must cite these.
- **Budget authority (resolved 2026-10-15):** spec §10's "Budget authority"
  paragraph governs — 500 MB cap-sum for Meeko's own processes (Codync excluded),
  idle ≤ 1.25× the measured baseline. PRD §4's 250 MB survives as the expected
  landing zone, not a competing ceiling. One number, per Architect + CR&S.

## 1. Environments

| Env | Use | Notes |
|---|---|---|
| Local (this machine) | U + C layers, primary | Verified present: Quickshell 0.3.2, hyprland, qmllint, qmltestrunner, cargo, node, python3. Suite commands must be discoverable by `ci/check.sh` so local == CI. |
| Headless Hyprland | E + P layers | `WLR_BACKENDS=headless`, nested compositor in container or local session. |
| Actions (optional) | mirror | Private repo → quota may block; gates must not depend on it (DevOps `ci/check.sh` parity). |

Every run records `qs --version`, `hyprland --version`, rustc/cargo/node versions in
the result header — failures get attributed to version drift, not guessed at.

**Measurement methodology (budget gates):** budget cases measure the two systemd
units' cgroups (`systemctl show -p MemoryCurrent quickshell.service
meeko-hookd.service`), never whole-session scanning — the Codync stack (≈378 MB,
`docs/qa/baseline-v0.md`) must stay visibly excluded from the number. RSS is the
median of N samples at a defined quiescence (5 min idle, N=5); this host runs near
its memory ceiling (~510 Mi available at last read, zram nearly full), so a single
uncontrolled sample would manufacture false failures. All swap-related tests run
against mocked `/proc/swaps` + zram sysfs, or dry-run — never the live swap
configuration.

## 2. Contract — hookd socket (spec §6.1)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| HOOKD-01 | connect → `snapshot` pushed | `{rev, link, agents, turns, approvals, usage, dropCount}` present, well-formed | BLOCKING |
| HOOKD-02 | `hello` | → snapshot, `{id,ok:true,result}` envelope | BLOCKING |
| HOOKD-03 | `approvals.list`, `agents.list`, `ping` | projection payload / `pong`; envelope shape exact | BLOCKING |
| HOOKD-04 | error envelope | any failure → `{id,ok:false,error:"…"}`, never a stack trace or empty line | BLOCKING |
| HOOKD-05 | foreign uid connects | refused by `SO_PEERCRED` uid==uid **before any parse**; connect logged without payload | BLOCKING |
| HOOKD-06 | observer calls shell-only verb | `{ok:false,error:"peer_not_shell"}` | BLOCKING |
| HOOKD-07 | malformed JSON line | `ok:false`, daemon survives, socket stays usable | BLOCKING |
| HOOKD-08 | line > 1 MiB | rejected at framing, before allocation blowup; daemon survives | BLOCKING |
| HOOKD-09 | per-verb caps | control verbs > 1 KiB rejected; `send.text` > 8 KiB rejected — enforced pre-parse | BLOCKING |
| HOOKD-10 | 8-connection cap | 9th concurrent connect refused/queued per spec, existing clients unaffected | BLOCKING |
| HOOKD-11 | dead client | 5 s idle read timeout → slot freed immediately; connect-with-kill-restart cycle never exhausts cap | BLOCKING |
| HOOKD-12 | zero clients | no buffering: RSS flat, projection only | BLOCKING |
| HOOKD-13 | socket path/mode | `${XDG_RUNTIME_DIR}/meeko-hookd.sock` mode 0600, parent dir 0700 | BLOCKING |
| HOOKD-14 | `MEEKO_HOOKD_SOCKET` override | validated identically to default (same mode/dir checks); override cannot weaken them | BLOCKING |
| HOOKD-15 | role resolution | `/proc/<pid>/exe` ↔ configured quickshell binary → `shell`, else `observer`; role labeled defense-in-depth (same-uid threat model, spec §12) | ADVISORY (documents CR&S accepted risk) |

## 3. Local validation gate — hookd is an allowlist gate, not a pipe (spec §6.1, §12)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| GATE-01 | `respondApproval` unknown entryId | `stale_entry`; mock host records **zero** method calls | BLOCKING |
| GATE-02 | `respondApproval` already-resolved entry | `stale_entry`; nothing proxied | BLOCKING |
| GATE-03 | `send` unknown botId | `unknown_bot`; nothing proxied | BLOCKING |
| GATE-04 | `stop` unknown botId | local reject; nothing proxied | BLOCKING |
| GATE-05 | verdicts for expired/foreign entries | never reach host (mock call log assertion) | BLOCKING |

## 4. hookd → Codync host client (spec §6.3)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| HOST-01 | default base URL | pinned `http://127.0.0.1:19222`; `CODYNC_PORT`-style override honored under guard | BLOCKING |
| HOST-02 | non-loopback URL, no `MEEKO_TEST=1` | hard startup error, daemon exits non-zero, no connection attempted | BLOCKING |
| HOST-03 | non-loopback + `MEEKO_TEST=1` | permitted (points at mock) — the only way | BLOCKING |
| HOST-04 | token file happy path | read, validated (uid==self, `mode & 0o077 == 0`, `O_NOFOLLOW`) | BLOCKING |
| HOST-05 | token file tampered: wrong owner / group+other bits / symlink swap | each rejected → read-only degraded mode, reason logged, **contents never logged** | BLOCKING |
| HOST-06 | `CODYNC_TOKEN_PATH` override | same validation as default path — no test-only bypass | BLOCKING |
| HOST-07 | 401 from host | token re-read (incl. mtime-change trigger); recovery without restart | BLOCKING |
| HOST-08 | token permanently invalid | read-only degraded mode, shell unaffected | BLOCKING |
| HOST-09 | token hygiene | dummy token value appears nowhere in socket payloads, argv, env, or logs (grep across artifacts) | BLOCKING |
| HOST-10 | SSE catch-up | `since=<rev>` replays events before live | BLOCKING |
| HOST-11 | rev gap > 5000 | full resync (snapshot re-pushed) | BLOCKING |
| HOST-12 | reconnect after host down | jittered exponential backoff 1 s → 30 s; no spin (connect-attempt rate measured) | BLOCKING |
| HOST-13 | `health.busy` | read verbs suppressed while busy | BLOCKING |
| HOST-14 | host down debounce | `link.offline` after 5 s debounce, not before | BLOCKING |

## 5. Projection bounds (spec §5)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| PROJ-01 | caps enforced | agents ≤ 100, turns ≤ 50/agent FIFO, approvals ≤ 10, usage ≤ 20 | BLOCKING |
| PROJ-02 | overflow behavior | drop oldest + `dropCount++` surfaced in every snapshot; over-cap projection = test failure | BLOCKING |
| PROJ-03 | preview truncation | `preview` ≤ ~120 chars | BLOCKING |
| PROJ-04 | never-cached fields | snapshot contains **no** entry bodies/files/memory/routines/teams/marketplace/credentials — asserted by absence, not trust | BLOCKING |
| PROJ-05 | `undecodable` event | projection marked stale → full resync; unknown shape → `link.degraded` (no guessing) | BLOCKING |

## 6. Approval flow end-to-end (spec §6.4.1)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| FLOW-01 | full path | mock `accessRequests` → `approval.add` → client `respondApproval` → mock receives `respondPermission {entryId, optionId}` exactly once → `approval.resolve` push | BLOCKING |
| FLOW-02 | double respond | second verdict locally rejected; exactly one host call total | BLOCKING |
| FLOW-03 | `optionId` optional | host call shape matches with/without it | BLOCKING |
| FLOW-04 | shell restart mid-flow | reconnect → fresh snapshot; no duplicate pushes | BLOCKING |

## 7. `qs ipc` surface (spec §6.2)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| IPC-01 | `meeko status` | `{ok, link, agents, approvals, dropCount}` | BLOCKING |
| IPC-02 | `meeko panel` | AgentPanel opens in island (E layer) | BLOCKING |
| IPC-03 | no verdict verbs | mechanical check: no respond/send/stop verb anywhere in the `qs ipc` surface (CR&S confused-deputy rule) | BLOCKING |
| IPC-04 | inherited verbs (Phase 1) | every `refs/impasto` `Commands.qml` verb still answers with identical JSON shapes — parity run against the reference clone | BLOCKING |

## 8. Quiet mode (spec §8)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| QUIET-01 | auto trigger | Hyprland fullscreen event (push) → `mode:"quiet"`, `source:"auto:fullscreen"`; no poll timer exists | BLOCKING |
| QUIET-02 | manual override | control-centre toggle wins over auto; resolution rule `manual > auto` holds | BLOCKING |
| QUIET-03 | repaint counter | `ShellModeService.repaintCount` goes **flat** in quiet; resumes on exit — the honest seam for "suspends waste" | BLOCKING |
| QUIET-04 | RSS drop | measurable RSS decrease entering quiet (P layer, with noise floor stated) | BLOCKING |
| QUIET-05 | exit fidelity | one coalesced frame applies latest state; full fidelity restored; **no catch-up burst** | BLOCKING |
| QUIET-06 | never suspended | notification queue, hookd socket live, projection stays current (events processed during quiet; one coalesced repaint after) | BLOCKING |
| QUIET-07 | beauty guard | interaction-driven motion while active untouched (ADVISORY visual check — animation smoothness report; beauty cannot be fully pixel-automated) | ADVISORY |

## 9. Lazy loading (spec §9)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| LAZY-01 | first frame has content | opened opt-in panel shows content on **first paint** (prewarm-on-intent ~150 ms works) | BLOCKING |
| LAZY-02 | no pop-in | no empty intermediate frame (loader status transitions observed) | BLOCKING |
| LAZY-03 | no layout shift | surface geometry stable across load | BLOCKING |
| LAZY-04 | resident-for-session | after first load, module stays resident (no unmount on close) | BLOCKING |
| LAZY-05 | unmount policy | un-mount only in quiet / memory pressure | BLOCKING |
| LAZY-06 | newcomer toggle | module enable/disable in settings, no config-file editing (E layer) | BLOCKING |

## 10. Budget & systemd (spec §10, §2)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| BUDGET-01 | idle flat | total-process RSS flat over 10 min (quickshell + hookd), excluding Codync processes | BLOCKING |
| BUDGET-02 | idle ceiling | quickshell-process idle ≤ ~85 MB (1.25 × 68 MB measured baseline, `docs/qa/baseline-v0.md`); hookd inside its own 64 MB cap | BLOCKING |
| BUDGET-03 | hard cap | quickshell `MemoryMax=430M` + hookd `MemoryMax=64M` = 494M < 500M — enforced by cgroup in a throwaway scope (container), never the user session | BLOCKING |
| BUDGET-04 | throttle before kill | `MemoryHigh=350M` engages before hard cap (sustained run stays well under) | BLOCKING |
| BUDGET-05 | crash recovery | `kill -9` quickshell → unit restarts within ~1 s, shell returns usable | BLOCKING |
| BUDGET-06 | cgroup pressure run | memory-limited run **degrades** (quiet path), never crash-loops | BLOCKING |
| BUDGET-07 | session env | unit binds `graphical-session.target`, imports `WAYLAND_DISPLAY`/`HYPRLAND_INSTANCE_SIGNATURE`/`XDG_RUNTIME_DIR` — fresh-login simulation in nested Hyprland; no black shell | BLOCKING |
| BUDGET-08 | loose coupling | hookd down (or absent) + codync-host down → shell == plain impasto; quickshell unit has no hard dependency on hookd | BLOCKING |
| BUDGET-09 | hookd independence | hookd starts and idles fine with `codync-host.service` stopped (degraded mode is the designed path) | BLOCKING |
| BUDGET-10 | unit vs exec-once | **blocked on §14.3** — BUDGET-03/05/07 assume the proposed user-unit; exec-once would remove the caps story entirely (flag to user if chosen) | BLOCKED |

## 11. Failure states (spec §11 — every bullet is a case)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| FAIL-01 | host down | no spinner, no error spam, no RSS creep; shell visually == plain impasto | BLOCKING |
| FAIL-02 | SSE reconnect storm | jittered backoff, bounded queue, no spin (attempt-rate metric) | BLOCKING |
| FAIL-03 | rev gap / over-cap | full resync; FIFO drop + `dropCount` (cross-ref PROJ-02) | BLOCKING |
| FAIL-04 | auth failures | 401 re-read; invalid token → read-only degraded (cross-ref HOST-07/08) | BLOCKING |
| FAIL-05 | socket abuse | foreign uid refused; observer verdict rejected; stale/unknown ids rejected locally; malformed JSON survives; dead client frees slot (cross-ref §2–3) | BLOCKING |
| FAIL-06 | quiet claims | counter flat + RSS drop + fidelity restored (cross-ref QUIET-03..05) | BLOCKING |
| FAIL-07 | pressure | cgroup-limited run degrades, never crash-loops (cross-ref BUDGET-06) | BLOCKING |

## 12. Security checks (spec §12; CR&S requirements)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| SEC-01 | fixture hygiene | every committed fixture synthetic or scrubbed: gitleaks **plus** content scan (no chat content, entry bodies, personal data); CR&S sign-off on first commit of each new fixture | BLOCKING |
| SEC-02 | no secrets in CI | dummy token only; `CODYNC_HOST_URL` points at mock; nothing for gitleaks to find (cross-ref HOST-09) | BLOCKING |
| SEC-03 | log redaction | no token, no chat content, no peer material in logs at any log level | BLOCKING |
| SEC-04 | confused-deputy | approvals unreachable via `qs ipc` (cross-ref IPC-03); verdicts ride Codync's authenticated channel only | BLOCKING |
| SEC-05 | role impersonation | same-uid non-quickshell binary posing as quickshell — current behavior (role granted) documented as accepted risk under the stated threat model; CR&S to confirm ADVISORY vs BLOCKING | ADVISORY |

## 13. Setup / doctor / onboarding (spec §10 swap policy, §14.2)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| SETUP-01 | swap option skippable | `--skip-swap` and interactive "no" both leave system untouched | BLOCKING |
| SETUP-02 | existing swap respected | pre-existing swap/hibernation detected → never overwritten (verify before/after `swapon --show`) | BLOCKING |
| SETUP-03 | dry-run | `--dry-run` (or equivalent) performs zero system changes | BLOCKING |
| SETUP-04 | graphical auth | root steps via pkexec; no terminal password prompt | BLOCKING |
| SETUP-05 | idempotent | running setup twice produces no diff | BLOCKING |
| SETUP-06 | env file generated | unit env file contains the three session vars (cross-ref BUDGET-07) | BLOCKING |
| SETUP-07 | firstrun/welcome | firstrun flag set once; welcome state shown once — **shape blocked on §14.2** (CLI vs in-shell) | BLOCKED |
| SETUP-08 | existing swap respected | `/proc/swaps` + `/sys/block/zram0/*` scanned **before** offering; existing zram (this box: 7.1/7.2 G, nearly full) or disk swap/hibernation → detected, printed with algo/size, creation step skipped, nothing touched; `--configure-zram` still refuses without `--force-zram` + printed current-vs-proposed diff | BLOCKING |
| SETUP-09 | doctor swap-health line | reports swap used %, zram fullness, available RAM; warns when zram > 80% — must fire on this box (98% full): correct signal, not noise | BLOCKING |

## 14. Skeleton, license, beauty (Phase 1; PRD §2)

| ID | Case | Pass criteria | Gate |
|---|---|---|---|
| SKEL-01 | module rename | `impasto` → `Meeko` everywhere; QML module id consistent; qmllint clean | BLOCKING |
| SKEL-02 | GPL headers | every new/modified file carries GPL-3.0 header (mechanical lint in CI) | BLOCKING |
| SKEL-03 | asset deny-list | no "coucou"/"Mochi" names, no Coucou sound assets anywhere in tree (grep gate) | BLOCKING |
| SKEL-04 | superseded modules removed | ClaudeModule, CodexModule, CommandCodeModule, OpenRouterModule, ImpastoMemoryModule absent | BLOCKING |
| SKEL-05 | Games/Pet cut | Games*/Pet* absent from repo | BLOCKING |
| SKEL-06 | beauty parity | island/bar render with zero QML warnings; visual diff vs impasto baseline (same theme) reported — **ADVISORY**, the human call | ADVISORY |
| SKEL-07 | verb parity | cross-ref IPC-04 | BLOCKING |

## 15. Fixtures & mock Codync host (QA-owned)

- **Mock host** (`tests/mock-codync/`, QA-owned): HTTP stub (`/health`, `/api/*`)
  + SSE server replaying scripted event sequences from the spec §3 families
  (`hello, bot, botDeleted, entry, usage, screen, accessRequests, cloud, resync,
  undecodable`); dummy token; records every received method call (the
  "nothing proxied" assertion of §3 depends on this log).
- **Env hooks consumed by suites**: `CODYNC_HOST_URL` → mock; `MEEKO_HOOKD_SOCKET`
  → test socket; `CODYNC_TOKEN_PATH` → fixture token; `MEEKO_TEST=1` for guard
  bypass (test-only, per HOST-02/03).
- **Fixture policy**: synthetic-first; any real recording scrubbed and signed off
  (SEC-01). Every fixture is a deterministic script → every failure report cites
  fixture id + rev sequence and is replayable.
- **Runners** (per layer): hookd → `cargo test`; QML units → `qmltestrunner`;
  pure JS → `node:test`; contract driver → Node script; E2E → headless Hyprland.
  Runners wired into `ci/check.sh` at Phase 2 start.

## 16. Coverage & gates (MEEKO-005 DoD)

- ≥ 80% on changed lines (hookd: cargo-llvm-cov/tarpaulin).
- QML has no coverage tooling → QML proxy = **contract surface coverage**: every
  verb and pushed event exercised with success *and* error shapes; the socket
  verb table in §6.1 is the coverage checklist (HOOKD/GATE rows = 100% required).
- BLOCKING cases gate the ticket; ADVISORY cases ship as a measured report the
  user can read.

## 17. Phase mapping (spec §15)

- **Phase 1** → SKEL, IPC-04/07. **Phase 2** → HOOKD, GATE, HOST, PROJ, FLOW,
  SEC, fixtures/mock. **Phase 3** → agent-surface cases (AgentModule chip +
  AgentPanel states rendered from projection — case set drafted at Phase 3 start).
  **Phase 4** → QUIET, LAZY, BUDGET, SETUP. FAIL matrix is continuous from
  Phase 2.

## 18. Blocked items (need decisions before those cases finalize)

1. §14.1 (widget engines / capture / launcher extras tier) → which modules LAZY
   cases cover.
2. §14.2 (onboarding shape) → SETUP-07.
3. §14.3 (systemd unit vs exec-once) → BUDGET-03/05/07 — the caps story depends
   on units; Architect's and DevOps' recommendation is units and the spec assumes it.
4. Number reconciliation PRD 250 MB vs spec 500 MB — **closed**: spec §10 Budget
   authority governs (§0); PRD §4 superseded.
5. Baseline capture — **landed** (`docs/qa/baseline-v0.md`): 68 MB quickshell →
   BUDGET-02 gate ≈ 85 MB.
