# Meeko — AIO Quickshell for Hyprland (design spec)

Date: 2026-10-15 · Status: for user review · Task: MEEKO-001/002/003

## 1. Purpose & intent

Meeko is an all-in-one desktop shell for Hyprland newcomers on constrained hardware
(< 8 GB RAM): GNOME-grade completeness and onboarding, one step ahead on
**compatibility, stability, and efficiency**, and the full visual identity of
impasto — shape-shifting island, palette-from-wallpaper, animations,
customisability — as a hard requirement, not decoration.

The differentiating feature: **AI agents as first-class desktop citizens**.
Meeko surfaces the user's already-running Codync agent host (bot roster, incoming
turns, approval cards, provider usage) inside the impasto shell. Coucou is the
**interaction-model reference only** (approval cards, notch/island behavior);
its code is NOT ported wholesale. Codync's Electron host is never embedded.

## 2. Non-negotiables

1. **Beauty of impasto preserved**: any change that visibly degrades the shell is
   a regression. Efficiency rules suspend waste, never motion.
2. **Shell budget ≤ 500 MB** total (shell + dots processes), excluding the
   already-running Codync processes. Enforced via systemd cgroup caps, not good
   intentions.
3. **Stable under memory pressure**: a leak or cap-hit throttles; it never
   starves a game and never crash-loops.
4. **No restart to game**: quiet mode is automatic (fullscreen) and manual
   (control-centre toggle).
5. **One new resident process only** (`meeko-hookd`); everything else in the
   dots layer is on-demand.

## 3. Constraints

- **License**: impasto base is GPL-3.0 → Meeko ships GPL-3.0. Coucou's name,
  Mochi, and sounds are NOT reused (MIT + asset copyright). Codync concepts are
  Apache-2.0 (compatible).
- **Repo**: private (decided). Reference clones under `refs/` are read-only
  inputs, not vendored into the repo.
- **Target host**: CachyOS + Hyprland, Wayland, quickshell.
- **Codync host API** (verified from source, `leepokai/Codync`):
  - systemd user service `codync-host.service`; `codync-host serve --port 19222`
    (dev 19223; `CODYNC_PORT` override).
  - `GET /health` → `{ok, hostId, computerId, version, busy, environment}`.
  - `POST /api/<method>`, `Authorization: Bearer <token from ~/.codync/token>`,
    loopback-only server-side (`is_loopback` + constant-time token compare).
  - `GET /events?since=<rev>&client=<id>` — SSE, catch-up then live. Event
    families: `hello`, `bot`, `botDeleted`, `entry`, `usage`, `screen`,
    `accessRequests`, `cloud`, `resync`, `undecodable`.
  - Methods used by Meeko: `hello`, `history`, `thread`, `send {botId,text,…}`,
    `respondPermission {entryId, optionId?}`, `stop {botId}`, `react`.
  - Approval answering is already in the host API — Meeko rides Codync's
    authenticated channel; verdicts never exist on `qs ipc call`.

## 4. Architecture

```
┌─ impasto shell (quickshell, QML, one resident process) ────────────┐
│  bar · island · launcher · dock · notifications · controls         │
│  + NEW: AgentModule (bar chip), AgentPanel (island: cards/turns)  │
│  + NEW: ShellModeService (active|quiet)                            │
│  + NEW: MeekoService (Quickshell.Io UnixSocket client)             │
└───────┬──────────────────────────▲─────────────────────────────────┘
        │ qs ipc (non-sensitive)   │ unix socket: NDJSON, 0600,
        │                          │ SO_PEERCRED, role check
┌───────▼──────────────────────────┴─────────────────────────────────┐
│  meeko-hookd (Rust daemon, ≤ 64 MB)                                │
│  • sole holder of the Codync token                                 │
│  • SSE subscriber + bounded in-memory projection                   │
│  • verb gate: local validation + role check before any host call   │
└───────┬────────────────────────────────────────────────────────────┘
        │ loopback HTTP/SSE (pinned 127.0.0.1:19222), bearer token
┌───────▼────────────────────────────────────────────────────────────┐
│  codync-host.service — already running, source of truth.           │
│  Meeko never embeds Electron, never persists agent state.          │
└────────────────────────────────────────────────────────────────────┘
```

Why hookd exists: QML has no HTTP/SSE client, and the token must never live in
QML. One small auditable daemon is the bridge and the security boundary.

## 5. Entity map (projection vs. host)

hookd's projection is **memory-only, bounded, never at rest**:

| Projection entity | Fed by | Bounds | Overflow |
|---|---|---|---|
| `Link {state, rev, hostVersion}` | hello/resync/health | — | — |
| `Agent {id, name, status, lastTs}` | bot / botDeleted | ≤ 100 | drop oldest, `dropCount++` |
| `Turn {id, agentId, preview, unread, ts}` | entry | ≤ 50 per agent, FIFO | drop oldest, `dropCount++` |
| `Approval {entryId, agentId, question, options[], ts}` | accessRequests | ≤ 10 | drop oldest, `dropCount++` |
| `Usage {provider, used, limit, resetsAt}` | usage | ≤ 20 | drop oldest, `dropCount++` |

`preview` is a truncated pointer to an entry (max ~120 chars); entry bodies,
files, memory, routines, teams, marketplace, credentials **are never cached** —
the host remains the source of truth. `undecodable` events (host newer than
hookd) mark the projection stale and force a full resync.

`dropCount` is surfaced in every `snapshot` so overflow is observable and
testable; a projection over cap is a test failure, not a silent shrink.

## 6. Contracts

### 6.1 hookd socket

- Path: `${XDG_RUNTIME_DIR}/meeko-hookd.sock`, mode `0600`, parent dir `0700`.
  Override `MEEKO_HOOKD_SOCKET` (validated identically in test and prod).
- Transport: newline-delimited JSON, one request/response pair per line pair;
  framing: `\n`-terminated, read timeout 5 s idle.
- Limits: ≤ 8 concurrent connections; line ≤ 1 MiB (hard); per-verb caps below;
  dead/idle clients dropped immediately (a restarted shell must never eat the
  cap).
- Auth on accept: `SO_PEERCRED` uid must equal hookd's uid — foreign uid is
  refused before any parse.
- Roles: hookd resolves `/proc/<pid>/exe` of the connecting pid; match against
  the configured quickshell binary ⇒ role `shell`, else `observer`.
  **The role check is defense-in-depth, not authentication.** Sensitive-verb
  protection rests on `0600 + SO_PEERCRED uid==uid` under the stated threat
  model (§12).
- Zero clients ⇒ buffering nothing. On connect: server pushes `snapshot`.

Client → hookd (verbatim):

| Verb | Params | Roles | Local gate before host call |
|---|---|---|---|
| `hello` | — | any | → snapshot |
| `approvals.list` | — | any | from projection |
| `agents.list` | — | any | from projection |
| `respondApproval` | `entryId`, `optionId?` | **shell only** | approval must exist in projection & pending; else `ok:false` `stale_entry` |
| `send` | `botId`, `text` (≤ 8 KiB) | **shell only** | bot must exist in projection; else `unknown_bot` |
| `stop` | `botId` | **shell only** | bot must exist in projection |
| `ping` | — | any | → `pong` |

Per-verb caps: control verbs (`hello`/`ping`/verdicts) ≤ 1 KiB, `send.text`
≤ 8 KiB — enforced before parsing the full line. Observer calling a shell-only
verb ⇒ `{ok:false, error:"peer_not_shell"}`. Envelope mirrors impasto's
Commands.qml: `{"id":42,"ok":true,"result":…}` / `{"id":42,"ok":false,"error":"…"}`.

hookd → client (pushed): `snapshot` (on connect / after full resync) ·
`agent.upsert` · `agent.remove` · `turn.add` · `approval.add` ·
`approval.resolve` · `usage` · `link`.

### 6.2 qs ipc (impasto `Commands.qml` pattern, non-sensitive only)

- `meeko status` → `{ok, link, agents, approvals, dropCount}`
- `meeko panel` → opens the AgentPanel in the island
- Verdict verbs never exist here (confused-deputy hole).

### 6.3 hookd → Codync host (internal client)

- Base URL pinned `http://127.0.0.1:19222`; non-loopback permitted only with
  `MEEKO_TEST=1`. Default port overridable via `CODYNC_HOST_URL` under the same
  guard.
- Token: read `~/.codync/token` (override `CODYNC_TOKEN_PATH`), validating
  owner uid == current uid, mode `& 0o077 == 0`, open with `O_NOFOLLOW`.
  Failure ⇒ start in degraded read-only mode, log the reason, never the
  contents. Re-read on 401 or file mtime change. Token never crosses the hookd
  socket, never in argv/env/logs.
- SSE `/events?since=<rev>`; rev gap > 5000 ⇒ drop projection, full resync.
  Reconnect: jittered exponential backoff 1 s → 30 s (no spin when host down).
  Read verbs suppressed while `health.busy`.
- Non-loopback URL without `MEEKO_TEST=1` ⇒ hard startup error.

### 6.4 Key flows

1. **Approval**: host `accessRequests` → hookd projects → `approval.add` push →
   QML renders Coucou-style card → user clicks → `respondApproval {entryId,
   optionId}` → role check + local pending check → host `respondPermission` →
   updated `accessRequests` → `approval.resolve` push.
2. **Degraded (host down)**: `link.offline` after 5 s debounce, backoff
   reconnect, projection frozen; shell otherwise behaves as plain impasto — no
   spinner, no error spam, no RSS creep.
3. **Shell restart**: reconnect → fresh `snapshot`. hookd tolerates zero
   clients by design.
4. **Host newer protocol** (`undecodable`): stale projection → resync; if the
   shape is unknown, surface `link.degraded` rather than guessing.

## 7. Module tiering & repo skeleton

Tiers (compatibility = everything available; efficiency = nothing costs until
used):

- **Core (always resident)**: bar, island, launcher, dock, notifications,
  controls, settings, `AgentModule`, `AgentPanel`, `ShellModeService`,
  `MeekoService`, lock screen.
- **Default-on (cheap, push-driven, no timers)**: clock, weather, battery,
  network, volume/brightness, media, stats, updates.
- **Opt-in (lazy-loaded on enable; zero idle cost otherwise)**: desktop widget
  engines (Analogue/Sticker), capture surface, launcher extras, SDDM theme.
- **Cut (removed from repo)**: Games*, Pet* (per user). The impasto dotfiles
  layer (nvim/btop/cava/yazi/kitty/gtk/…) is NOT part of Meeko — it stays
  personal dotfiles; Meeko ships only the shell + Hyprland config + setup.

```
meeko/
├── home/.config/
│   ├── quickshell/                # impasto QML core, module id renamed `Meeko`
│   │   ├── shell.qml · Commands.qml (+ meeko status/panel verbs)
│   │   ├── services/              # inherited services +
│   │   │   ├── ShellModeService.qml     # NEW active|quiet state, repaint counter
│   │   │   └── MeekoService.qml         # NEW socket client, projection mirror
│   │   ├── bar/island/AgentPanel.qml    # NEW approval cards, turns, usage
│   │   ├── bar/modules/AgentModule.qml  # NEW status chip (+Claude/Codex usage REMOVED)
│   │   └── bar/·theme/·settings/·lock/·dock/·desktop/  (inherited)
│   ├── hypr/                      # inherited Lua config (+ fullscreen event wiring)
├── hookd/                         # NEW Rust daemon
│   └── src/{main,config,token,host,projection,socket,verbs,events}.rs
├── system/systemd/user/{quickshell.service,meeko-hookd.service}
├── setup                          # first-run: packages, swap option, units, env file, firstrun flag
├── ci/check.sh                    # local mirror of CI gates (qmllint, syntax, hookd tests, gitleaks)
├── tests/                         # QA-owned: fixtures (scrubbed SSE) + suites
└── docs/
```

The QML module rename (`impasto` → `meeko`, GPL headers updated) is a
mechanical Phase-1 change; visual output must be pixel-compatible.

Superseded by Meeko (removed at Phase 1): per-provider usage pollers
(CommandCodeModule, OpenRouterModule, ClaudeModule, CodexModule,
ImpastoMemoryModule) — Codync `usage` events replace all of them via one
connection.

## 8. ShellModeService & quiet mode

- State: `mode: "active" | "quiet"`, `source: "auto:fullscreen" | "manual"`,
  plus `repaintCount` (monotonic counter bumped by suspending surfaces — QA's
  testable seam, cheap now / awkward later).
- Resolution: `manual override` if set, else fullscreen ? quiet : active.
- Triggers (push, never poll): Hyprland fullscreen event via the existing
  HyprlandService; manual toggle in the control centre.
- **Quiet suspends**: animation drivers (island/launcher/cards freeze
  mid-motion), GPU surfaces unmounted (spectrum shader, sheen/blur), non-essential
  panels stop repainting (bar stays, static), MeekoService keeps the socket
  live (projection current; at most one coalesced repaint on exit).
- **Quiet never suspends**: interaction-driven motion while active, the
  notification queue, hookd itself.
- Exit quiet: one coalesced frame applies latest state; full visual fidelity
  restored; no catch-up burst.
- Surfaces declare quiet behavior through the service (one state machine, not
  per-panel hacks).

## 9. Lazy loading (opt-in tier)

- `Loader { asynchronous: true }` per module; **prewarm on intent**
  (hover/focus/keybind/panel-open) ~150 ms before reveal so first paint has
  content — no pop-in, no layout shift.
- Once loaded, a module stays resident for the session (first-open cost paid
  once). Un-mount only in quiet mode or memory pressure, where a re-mount hitch
  is acceptable.
- Per-module load triggers are listed in the spec so QA can assert
  "opened panel shows content on first frame".
- Newcomer toggle in settings, no config editing.

## 10. Process budget & systemd

| Process | MemoryHigh | MemoryMax | Restart |
|---|---|---|---|
| quickshell (user unit) | 350M | 430M | on-failure, 1 s |
| meeko-hookd (user unit) | — | 64M | on-failure, 1 s |
| python helper scripts | on-demand only (never resident) | | |

Hard-cap arithmetic: 430M + 64M = **494M < 500M** — the never-exceed line stays
inside the promise. `MemoryHigh` throttles quickshell at 350M before the hard
cap engages, so sustained running stays well below it.

**Session environment (required for the unit):** a user service does not inherit
the compositor session env. Units bind to `graphical-session.target` (i.e. after
Hyprland), import `WAYLAND_DISPLAY`, `HYPRLAND_INSTANCE_SIGNATURE`,
`XDG_RUNTIME_DIR` via `ImportEnvironment` or an env file generated at
`meeko setup`, and stop cleanly at logout. Without this the shell starts black
on a fresh install — it's part of the AIO promise, not an optional extra. The
quickshell unit takes **no hard dependency on meeko-hookd**: if the daemon is
down, MeekoService's degraded mode is the designed path.

**Budget authority (single source of truth; supersedes PRD §4's early
≤250 MB figure):** the never-exceed line is the systemd cap sum — **500 MB total
for Meeko's own processes** (quickshell 430M + hookd 64M = 494M), with Codync's
processes excluded. The pass/fail idle gate is **≤1.25× the measured
impasto-only baseline**; at a ~250 MB baseline that lands ≈310 MB idle, which
is how the earlier 250 MB figure survives — as the expected idle landing zone,
not a second ceiling. All budget tests assert this section.

hookd has **no ordering dependency** on `codync-host.service` (it must run when
the host is down — degraded mode). `Restart=on-failure` makes worst-case kills
self-healing.

**Swap/zram policy** (per user, now in PRD): `meeko setup` offers a swap/zram
step as a *recommended but skippable option* on low-RAM machines. It must
detect an existing swap/hibernation configuration and respect it — never
overwrite — and support dry-run. Memory config is informed and user-chosen,
not forced. Root changes go through graphical authorization (pkexec), never a
terminal password prompt.

Shell total target: ≤ 500 MB including margin; QA gates idle ≤ 1.25× the
user's measured impasto-only baseline (baseline capture: System agent).

## 11. Degraded mode & failure states (pass/fail for QA)

- codync-host down ⇒ shell == plain impasto (no spinner, no spam, no creep)
- SSE reconnect storm ⇒ jittered backoff, bounded queue, no spin
- rev gap > 5000 ⇒ full resync; over-cap projection ⇒ FIFO drop + `dropCount`
- 401 / token rotation ⇒ re-read; token invalid ⇒ read-only degraded mode
- socket: foreign uid ⇒ refused; observer ⇒ `peer_not_shell` on verdicts;
  stale/unknown ids ⇒ local reject, nothing proxied; malformed JSON ⇒
  `ok:false`, daemon survives; dead client ⇒ slot freed
- quiet mode ⇒ repaint counter flat, RSS drop measured, fidelity restored on exit
- idle budget: total-process RSS flat over 10 min; cgroup-limited run degrades,
  never crash-loops

## 12. Security & threat model

- Approvals and agent actions are **same-uid trusted**: protections defend
  against *other users, sandboxed webviews, and remote/agent callers*, not a
  fully compromised user session. The `/proc/<pid>/exe` role check is
  defense-in-depth only (§6.1).
- hookd is the sole token holder; no token on the socket, no credentials at
  rest, redacted logs (no chat content, no tokens, no peer material).
- Every verb validates against the local projection before any host call —
  hookd is an allowlist gate, not a pipe.
- Fixtures are synthetic/scrubbed; gitleaks + content scrub in CI; no secrets
  in Actions (dummy token only, `CODYNC_HOST_URL` mock).

## 13. Out of scope (v1)

- Embedding or forking the Codync Electron host; patching Codync's provider
  layer (CommandCode/OpenRouter native ACP) — separate project.
- Codync screens, voice, marketplace, routines, Android/iOS companions.
- Any change to the Codync host itself. Meeko depends on its loopback API
  stability; version drift is handled by `undecodable`/resync, not vendoring.

## 14. Open decisions (user)

1. Final opt-in vs cut for: desktop widget engines, capture surface, launcher
   extras, lock screen prominence. **Proposed**: widgets + capture opt-in,
   lock stays core (it's part of the impasto identity), launcher extras opt-in.
2. First-run onboarding shape: in-shell welcome panel vs CLI `setup` walkthrough
   (proposed: CLI `setup` + in-shell "welcome" state flag for v1).
3. Exact systemd scope: quickshell as user unit (enables caps + auto-restart) vs
   Hyprland exec-once. **Proposed**: user unit — the caps are the point.

## 15. Phasing (for the implementation plan)

1. **Phase 1 — skeleton**: repo init, impasto import, QML rename, GPL headers,
   remove superseded modules + Games/Pet, inherited `qs ipc` verbs green.
2. **Phase 2 — hookd**: socket + host client + projection + degraded mode,
   contract tests vs mock.
3. **Phase 3 — agent surface**: AgentModule, AgentPanel, approval cards, usage chip.
4. **Phase 4 — efficiency**: ShellModeService, quiet mode, tiering + lazy load,
   systemd units + caps, setup (packages, swap option, session env file),
   onboarding.
