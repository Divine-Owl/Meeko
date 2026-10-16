# Meeko Phase 1 — Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import impasto's quickshell tree as Meeko's skeleton, rename it `impasto`→`meeko`, strip the superseded poller modules and Games/Pet, and add the two new `qs ipc` verbs from spec §6.2 (`meeko status`, `meeko panel`) — all mechanical, no behavior change, pixel-compatible output.

**Architecture:** Phase 1 is the import/cleanup half of spec §7's repo skeleton: `home/.config/quickshell/` (QML core, module id `Meeko`), inherited Hyprland Lua config, scripts, and system files. One new process (`meeko-hookd`) and the agent surface (AgentModule/AgentPanel/ShellModeService) land in later phases; Phase 1 deliberately adds nothing at runtime beyond the two verb stubs, so CR&S's mechanical-only gate can verify the diff by inspection.

**Tech Stack:** QML/Quickshell 0.3.2 (imported verbatim), Lua (Hyprland config, inherited), Python usage scripts (inherited, on-demand only), systemd user units (Phase 4), Rust hookd (Phase 2). Tests: `qmllint`, static contract assertions, `git diff --stat` for the mechanical-only review.

**Spec:** `docs/superpowers/specs/2026-10-15-meeko-shell-design.md` — committed at `4c11003`, amended at `28aa687`. The plan argues from it; executors read both.

## Global Constraints

(Every task implicitly includes these; values copied verbatim from spec §2–§10.)

1. **License:** Meeko ships GPL-3.0 (impasto base). Coucou's name, Mochi, and sounds are never reused. Codync concepts are Apache-2.0-compatible (spec §3).
2. **Repo:** private; `refs/` clones are read-only inputs, never vendored (spec §3).
3. **Target host:** CachyOS + Hyprland + Wayland + quickshell. QML shell only — no Electron, Tauri, or Swift embedded (spec §3, §13).
4. **Beauty:** any visible degradation of impasto's shell is a regression; efficiency suspends waste, never motion (spec §2, §8).
5. **Budget:** quickshell MemoryHigh 350M / MemoryMax 430M; hookd MemoryMax 64M; cap-sum 494M < 500M never-exceed (spec §10). Phase 1 touches no units — this is a constraint on later phases, but the rename must not add resident code.
6. **Idle gate:** ≤1.25× measured baseline (≈85 MB from the 68 MB capture in docs/qa/baseline-v0.md); Codync's ≈378 MB excluded (spec §10).
7. **Codync host API pinned:** `http://127.0.0.1:19222`; token from `~/.codync/token` with uid+0600+`O_NOFOLLOW` validation (spec §6.3). Phase 1's `meeko status` stub reports `link.state: "offline"` — no host client yet.
8. **Socket contract:** `${XDG_RUNTIME_DIR}/meeko-hookd.sock`, mode 0600, parent 0700, `\n`-terminated NDJSON, ≤8 connections, 1 MiB line, 5 s idle read timeout (spec §6.1). Phase 1 adds no socket code — constraint on Phase 2.
9. **Envelope:** every IPC answer is a JSON line `{"ok": true, …}` or `{"error": "…"}` mirroring impasto's Commands.qml `ok()`/`fail()` (spec §6).
10. **Commit split (CR&S gate):** four separate commits — (1) verbatim import, (2) rename pass, (3) deletions + reference sweep, (4) new verbs + static tests (spec §6.2; Code Review & Security pass 1–3).

## Review Focus

Failure modes the spec implies but no Phase 1 task's tests directly exercise, with the owning task and pinning test:

1. **Orphaned references after deletion** — removed poller/Games/Pet files leave dangling `qmldir` entries, autostart entries, imports, or `qs ipc` verbs; shell fails to load. Pinned by Task 3's reference sweep + `qmllint` on the whole tree (Task 3).
2. **Rename bleeding into logic lines** — only identifiers, module ids, wording, and headers change; any added/removed logic line in inherited services is a CR&S reject unless justified in the PR. Pinned by CR&S pass 1 on Task 2's commit (Task 2).
3. **New verbs shadowing existing targets** — `meeko` must not collide with any existing IpcHandler target or change any existing target's signature. Pinned by Task 4's verb-baseline test enumerating every pre-existing target (Task 4).
4. **qmldir module-id mismatch** — QML files `import "…"` the qmldir module id; a renamed module id with stale imports breaks loading. Pinned by `qmllint` exit 0 on every QML file after Task 2 (Task 2).
5. **Visual regression** — any rendered output changed by the import/rename. Pinned by no-op diff of all non-identifier, non-header lines in Task 2 and Task 3's logic-line accounting (Tasks 2–3).

## Phase 1 Tasks

### Task 1: Verbatim import of the impasto quickshell skeleton

**Files:**
- Create: `home/.config/quickshell/` — entire tree from `refs/impasto/home/.config/quickshell/` (shell.qml, Commands.qml, qmldir, services/, bar/, components/, settings/, desktop/, dock/, lock/, deck/, capture/, theme/, scripts/)
- Create: `home/.config/hypr/` — Hyprland Lua config from `refs/impasto/home/.config/hypr/`
- Create: `scripts/` — impasto's Python usage scripts from `refs/impasto/home/.config/quickshell/scripts/` (on-demand only per spec §5; never resident)
- Create: `system/` — polkit actions, sdm theme, PAM config from `refs/impasto/system/`
- Modify: `.gitignore` — add `refs/` (read-only reference clones, never vendored) if not already present
- Test: `tests/qa/phase1/import.tree.test.sh` — asserts every file listed in `refs/impasto/home/.config/quickshell/` exists byte-identical under `home/.config/quickshell/`

**Interfaces:**
- Consumes: `refs/impasto/` (read-only reference clone; exact file inventory from `repo_tree` output)
- Produces: complete `home/.config/quickshell/` tree; `qmllint` passes on every QML file

- [ ] **Step 1: Copy `refs/impasto/home/.config/quickshell/` → `home/.config/quickshell/` verbatim**

  Run: `cp -a refs/impasto/home/.config/quickshell/. home/.config/quickshell/` (plus hypr/, system/ trees)
  Expected: tree present; `find home/.config/quickshell -type f | wc -l` matches `refs/impasto` count

- [ ] **Step 2: Verify byte-identity of the import**

  Run: `tests/qa/phase1/import.tree.test.sh`
  Expected: PASS — every file identical, no additions, no omissions

- [ ] **Step 3: Lint the imported tree**

  Run: `qmllint home/.config/quickshell/**/*.qml` (or per-file equivalent; qmllint on the directory)
  Expected: exit 0, no QML errors

- [ ] **Step 4: Commit the import**

  ```bash
  git add home/.config/quickshell/ home/.config/hypr/ scripts/ system/ .gitignore tests/qa/phase1/import.tree.test.sh
  git commit -m "feat(skeleton): verbatim import of impasto quickshell tree"
  ```
  Expected: clean commit on `main`, tree clean afterwards

### Task 2: Rename pass — impasto → meeko (identifiers + headers only)

**Files:**
- Modify: `home/.config/quickshell/qmldir` — `module QuickshellConfig` → `module Meeko` (line 1); no other qmldir changes in this task (entries added/removed in Task 3)
- Modify: `home/.config/quickshell/Commands.qml` — header comment references impasto → Meeko; any `impasto` identifier in QML code (not prose comments) renamed to `meeko`; e.g. `theme list` wording "(impasto theme list)" → "(meeko theme list)"
- Modify: `home/.config/quickshell/bar/Bar.qml`, `bar/BarReserve.qml`, `bar/island/*.qml`, `components/*.qml`, `shell.qml`, `settings/*.qml`, `scripts/*.py`, `hypr/*.lua`, `system/*` — identifiers/comments referencing `impasto` → `meeko` where they name the project/module; prose and wallpaper paths untouched

**Interfaces:**
- Consumes: Task 1's verbatim tree
- Produces: same tree, every `impasto` project identifier → `meeko`; QML semantics unchanged

- [ ] **Step 1: Rename qmldir module id**

  Modify: `home/.config/quickshell/qmldir:1` — `module QuickshellConfig` → `module Meeko`
  Expected: line 1 reads exactly `module Meeko`

- [ ] **Step 2: Rename identifiers in QML + scripts + config**

  Run: project-wide replace of code-identifier `impasto` → `meeko` in `home/.config/quickshell/**`, `home/.config/hypr/**`, `scripts/`, `system/` (comments describing the project may keep either spelling; only identifiers, module ids, and headers change)
  Expected: no `import` or `module` reference to `QuickshellConfig`/`impasto` identifier remains as code; `qmllint` still passes

- [ ] **Step 3: Update file-level headers to GPL-3.0 / Meeko**

  Modify: any header block naming impasto (e.g. Commands.qml header) → Meeko project, GPL-3.0
  Expected: headers name Meeko and GPL-3.0; no Coucou name/Mochi/sounds referenced anywhere

- [ ] **Step 4: Lint + diff review**

  Run: `qmllint home/.config/quickshell/**/*.qml`
  Expected: exit 0
  Run: `git diff HEAD --stat` on the rename commit — logic-line churn must be ~0 (only identifiers/headers/wording)
  Expected: no added/removed `function`, `signal`, `property`, or `IpcHandler` lines

- [ ] **Step 5: Commit the rename**

  ```bash
  git add home/ scripts/ system/ tests/qa/phase1/import.tree.test.sh
  git commit -m "refactor(quickshell): rename impasto → meeko identifiers and headers"
  ```
  Expected: clean commit; mechanical-only diff verifiable by CR&S pass 1

### Task 3: Delete superseded modules (poller modules + Games/Pet) + reference sweep

**Files:**
- Delete: `home/.config/quickshell/services/ClaudeService.qml`, `services/CodexService.qml` (superseded pollers — Codync `usage` replaces them, Phase 3)
- Delete: `home/.config/quickshell/bar/modules/ClaudeModule.qml`, `bar/modules/CodexModule.qml`, `bar/modules/GamesModule.qml`, `bar/modules/PetModule.qml`
- Delete: `home/.config/quickshell/services/GamesService.qml`, `services/PetService.qml`
- Delete: `home/.config/quickshell/bar/island/games/*.qml` (Bots, Burst, Flood, GameBoard, Hextris, Lights, Mini2048, Pop, Snake, Solitaire, Space, Target, Tetris, Whack)
- Delete: `home/.config/quickshell/bar/island/PetPanel.qml`, `bar/island/GamesPanel.qml`
- Delete: `home/.config/quickshell/components/pets/*.qml` (PetCreature, PetEgg, PetPaper, PetPixel, PetPlush, PetSoftFace), `components/PetFace.qml`, `components/PetFamily.qml`, `components/PetShelf.qml`
- Modify: `home/.config/quickshell/qmldir` — remove all entries for deleted files (ClaudeModule, CodexModule, GamesModule, PetModule, ClaudeService, CodexService, GamesService, PetService, PetFace, PetFamily, PetShelf, GamesPanel, PetPanel, games/*.qml entries if present; ImpastoBlock/PetBlock/GamesBlock entries only if they exist in this tree — delete entry only if file deleted)
- Modify: any file importing deleted modules/services (sweep `import` statements, `qmldir` autostart references, `Commands.qml` verb list entries pointing at deleted code — the `game` target stays only if it references no deleted code, else remove with a reference-cleanup note)

**Interfaces:**
- Consumes: Task 2's renamed tree
- Produces: tree minus poller modules and Games/Pet surfaces; no dangling references; `qmllint` exit 0; `qs ipc` verb list (Commands.qml panel target) unchanged except deletions it forces

- [ ] **Step 1: Delete the poller modules and services**

  Run: `git rm` the six files listed above
  Expected: files gone; `grep -rn "ClaudeService\|CodexService" home/` finds nothing

- [ ] **Step 2: Delete Games/Pet surfaces**

  Run: `git rm` games/, PetPanel.qml, GamesPanel.qml, components/pets/, PetFace.qml, PetFamily.qml, PetShelf.qml
  Expected: no `Pet*`/`Games*` QML file remains under `home/.config/quickshell/`

- [ ] **Step 3: Sweep qmldir + imports for dangling references**

  Modify: `home/.config/quickshell/qmldir` — remove entries for every deleted file
  Run: `grep -rn "ClaudeModule\|CodexModule\|GamesModule\|PetModule\|PetService\|GamesService" home/`
  Expected: no matches; `qmllint` exit 0

- [ ] **Step 4: Sweep autostart, settings panels, Commands.qml verb list**

  Run: audit `home/.config/hypr/modules/autostart.lua`, `settings/*`, `Commands.qml` panel names list for references to deleted modules; delete or neutralize any dangling reference, each with a reference-cleanup note
  Expected: zero dangling references; every remaining `qs ipc` target still resolves to an existing handler

- [ ] **Step 5: Lint + commit deletions**

  Run: `qmllint home/.config/quickshell/**/*.qml`
  Expected: exit 0
  ```bash
  git add -A home/
  git commit -m "chore(quickshell): remove superseded pollers and Games/Pet surfaces"
  ```
  Expected: clean commit; deletion message notes every reference cleaned up (CR&S pass 2)

### Task 4: Add §6.2 verbs (`meeko status`, `meeko panel`) + verb-baseline test

**Files:**
- Modify: `home/.config/quickshell/Commands.qml` — add `meeko` IpcHandler with `status()` and `panel()` functions
- Create: `tests/qa/phase1/verbs.static.test.sh` — static contract test

**Interfaces:**
- Consumes: Task 3's cleaned tree
- Produces: `meeko status` → `{"ok": true, "link": {"state": "offline", "rev": 0, "hostVersion": null}, "agents": [], "approvals": [], "dropCount": 0}`; `meeko panel` → `{"ok": true}` no-op (AgentPanel lands Phase 3); every pre-existing IpcHandler target/verb present with unchanged signatures

- [ ] **Step 1: Write the failing verb-baseline test**

  Create: `tests/qa/phase1/verbs.static.test.sh` asserting:
  - every pre-existing target from the baseline Commands.qml exists (status, timer, note, task, panel, machine, media, capture, game, layout, focus, nightlight, bar, widgets, volume, brightness, theme, wallpaper, osd, lock, shell) with unchanged function names/signatures
  - new `meeko` target exists with `status` and `panel` functions
  - `meeko status` shape contains keys `link`, `agents`, `approvals`, `dropCount`
  - `meeko panel` returns `ok`
  Run: `tests/qa/phase1/verbs.static.test.sh`
  Expected: FAIL — `meeko` target not yet in Commands.qml

- [ ] **Step 2: Add the `meeko` IpcHandler to Commands.qml**

  Modify: `home/.config/quickshell/Commands.qml` — insert `meeko` IpcHandler:

  ```qml
  IpcHandler {
      target: "meeko"

      function status(): string {
          return root.ok({
              link: { state: "offline", rev: 0, hostVersion: null },
              agents: [],
              approvals: [],
              dropCount: 0
          })
      }

      function panel(): string {
          // AgentPanel lands in Phase 3; stub keeps the verb shape stable.
          return root.ok()
      }
  }
  ```
  Expected: `status()` returns the envelope shape above; `panel()` returns `{"ok": true}`; no existing target modified

- [ ] **Step 3: Verify verb baseline passes**

  Run: `tests/qa/phase1/verbs.static.test.sh`
  Expected: PASS
  Run: `qmllint home/.config/quickshell/Commands.qml`
  Expected: exit 0

- [ ] **Step 4: Commit the new verbs**

  ```bash
  git add home/.config/quickshell/Commands.qml tests/qa/phase1/verbs.static.test.sh
  git commit -m "feat(quickshell): add meeko status/panel ipc verbs"
  ```
  Expected: clean commit; CR&S pass 3 verifies baseline intact plus exactly two new verbs

### Task 5: Phase 1 review gate (not implementation — verification only)

**Files:** none new; runs against Tasks 1–4 commits

**Interfaces:**
- Consumes: branch `feature/MEEKO-003-skeleton` after Task 4
- Produces: CR&S-ready diff for mechanical-only review

- [ ] **Step 1: Present the four-commit split for CR&S passes**

  Run: `git log --oneline main..HEAD` (after Git Operator cuts `feature/MEEKO-003-skeleton`)
  Expected: exactly four commits: import → rename → deletions → new verbs (+ static test commit folded into Task 4's commit or separate test commit)

- [ ] **Step 2: CR&S pass 1 (rename audit)**

  Run: review Task 2's diff — identifier/wording/header changes only; no logic lines
  Expected: CR&S signals mechanical-only; any logic line justified or reverted

- [ ] **Step 3: CR&S pass 2 (deletion audit)**

  Run: review Task 3's diff — no orphaned `qmldir` entries, imports, autostart entries, or verbs pointing at deleted files
  Expected: clean sweep, documented per-commit

- [ ] **Step 4: CR&S pass 3 (verb baseline)**

  Run: Task 4's static test + diff of Commands.qml targets
  Expected: all baseline targets unchanged; exactly `meeko`/`status`/`panel` added

- [ ] **Step 5: Hand off**

  Report: Phase 1 complete, `feature/MEEKO-003-skeleton` ready for Phase 2 (hookd) and QA contract suites riding behind the first commit.

## Self-review against the spec

- §7 skeleton: import + rename + deletions + two §6.2 verbs covered (Tasks 1–4).
- §6.2 `meeko status`/`meeko panel`: present as stubs; host wiring deferred to Phase 2/3 per spec (hookd has no Phase 1 code).
- §5 entity map: `status()` shape matches `link`/`agents`/`approvals`/`dropCount`; host-fed values arrive in Phase 2.
- §3 license: GPL-3.0 headers; no Coucou assets referenced.
- §10 budget: no resident code added in Phase 1 (stubs only); caps belong to Phase 4 units.
- Commit split matches CR&S's three audit passes exactly (Tasks 2–4 + Task 5 verification).

Plan complete and saved to `docs/superpowers/plans/2026-10-16-meeko-phase1-skeleton.md`. Please review the plan. Does it capture what you want?
