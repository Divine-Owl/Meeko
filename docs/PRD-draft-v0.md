# Meeko — Merged Quickshell for Hyprland (PRD draft v0)

Status: DRAFT — awaiting user decisions on blocking questions (see §6)

## 1. Vision
One synergistic Hyprland desktop shell combining:
- **impasto** (GPL-3.0) — the Quickshell QML island/bar/launcher base, taken whole.
- **coucou** (MIT) — notch/AI-companion concepts: approval cards, agent status, dictation hooks. Swift/AppKit is rewritten in QML; portable Rust (`HookServer`, `secrets.rs`) is ported. Name "coucou", Mochi, and sounds are NOT reused (© the author).
- **Codync** (Apache-2.0) — agent-session model and hook conventions, not the Electron app.

## 2. License & compliance
- Merged work ships **GPL-3.0** (impasto is the base; MIT + Apache-2.0 are compatible).
- No reuse of Coucou's name, Mochi character, or sound assets.
- All inherited code keeps original copyright headers; new files get GPL-3.0 headers.

## 3. Architecture (Developer's recommendation, pending Architect sign-off)
- **QML shell** (impasto skeleton, renamed to Meeko) — interpreted, no compile step.
- **Exactly one native helper: `meeko-hookd`** (Rust) — agent-hook intake (Coucou HookServer-derived) + keyring access (Coucou secrets.rs-derived: Secret Service/KWallet behind KNOWN_KEYS allowlist, presence-only queries from UI).
- No Electron, no Tauri, no Swift at runtime. Keeps idle RSS inside QA's budget.
- **Security boundary:** approval verdicts are NEVER `qs ipc call` verbs. Sensitive verbs live on `meeko-hookd`'s Unix socket with `SO_PEERCRED` same-uid + policy checks. Human-in-the-loop approval is the default grant path.

## 4. Non-functional requirements
- **Budget authority (superseded by spec §10, 2026-10-15):** never-exceed = **500 MB total for Meeko's own processes** (Codync excluded, systemd cgroup caps); idle gate = **≤1.25× measured baseline** (~68 MB quickshell baseline → ≈85 MB idle landing zone, hookd within its 64 MB cap). The old 250 MB figure is retired as a ceiling; QA asserts spec §10 verbatim.
- Flat RSS: no growth over 10 min (QA gate, pass/fail).
- Every IPC verb ships JSON success AND error shapes (`{"ok":true,...}` / `{"error":"..."}`), input-validated per impasto's existing pattern (allowlists, parseInt bounds, regex ids).
- HookServer limits carried over: 1 MB payload cap, recv timeout, 32-connection ceiling.
- Polkit helpers (if any) follow impasto's argv1-annotation + `auth_self_keep` + sanitized-input pattern.

## 5. Test strategy (QA's pyramid)
1. Unit — pure logic (date parsing, switched() semantics, config merge) under qmltestrunner/Node, hermetic.
2. Contract/integration — spawn shell, round-trip every target/verb, assert success AND error JSON.
3. E2E smoke — headless Hyprland (`WLR_BACKENDS=headless`) in Arch container: boots, panels load, inherited verbs respond, RSS/CPU thresholds hold.

## 6. Decisions (from user, 2026-10-15)
1. **Center of gravity: Option A (shell-first), Codync-first agent module.** User runs impasto Quickshell on Hyprland/CachyOS already; the merge's purpose is **Codync integration in the bar** — agent status, turns, approvals visible and interactable from the existing island. Coucou is the *UX reference* for the notch/approval-card interaction model, not a logic dependency. Design with a seam toward Option C (co-equal agent surface) for later.
2. **Idle footprint is a hard user concern** — the shell must stay a thin QML client + one small Rust bridge; it must NOT embed Codync's Electron host. Meeko talks to the running Codync instance over its API/socket; idle shell RSS stays ≤250 MB.
3. **Out of scope:** adding CommandCode/OpenRouter as native ACP providers to Codync (separate Codync-side project; Meeko surfaces whatever agents Codync already runs).
4. **Repo: private** (Git Operator/DevOps: quota-limited CI; unit+contract layers run anywhere).
5. **Local refs cloned (read-only, no builds here):** `refs/impasto` (368M), `refs/coucou` (50M), `refs/Codync` (27M) — shallow clones, source-of-truth for all agents.
6. **Product vision (user, decisive): Meeko = the AIO for Hyprland newcomers** — GNOME-quality onboarding/defaults, but one step ahead on **compatibility, stability, efficiency**. Ships as shell + dots + setup/doctor, not just config.
7. **Feature triage (user):** keep core + supersede provider-usage modules with the Codync agent module (user's own CommandCode/OpenRouter/ImpastoMemory modules + upstream Claude/Codex usage modules all deprecated → one hookd-fed surface); **cut Games and Pet**; desktop-widget themes, launcher extras, lock screen, desk dotfiles = UNDECIDED pending further discussion.
8. **HARD memory constraint: the whole shell/dots stack (quickshell + hookd + helpers, EXCLUDING Codync host) must stay ≤ 500 MB RAM** — user runs an <8 GB machine and must be able to game with Meeko running, no restart-the-shell ritual.
9. **Beauty is a non-negotiable requirement (user):** impasto's customisability, animations, design language and functionality must NOT be compromised. Efficiency rules kill *waste* (idle timers, always-on shaders, unnecessary repaints), never *motion* — interactive animation, the island's shape-shifting, the palette-from-wallpaper system and the widget theme engines are part of the product. This tilts the "undecided" cut list toward KEEP (as polished, possibly off-by-default modules) over CUT. Regression bar: Meeko must look and feel like impasto, not a diet version.
10. **Swap/zram = an option, not forced (user):** `meeko setup` offers a swap/zram step — recommended on <8 GB boxes, skippable, never overwrites an existing swap/hibernation setup. Auto-detect, inform, respect the user's answer.
11. **Phase 0 CLOSED (user, 2026-10-16: "looks good, proceed").** §14 decisions, all approved as recommended: (a) widget engines / capture surface / launcher extras = **opt-in modules**; (b) onboarding = **CLI setup walkthrough + in-shell welcome panel**; (c) quickshell runs as a **systemd user unit** (graphical-session.target, session-env import, cgroup caps) — not exec-once. Phase 1 may begin after writing-plans.

## 7. Task manifest (v0)
| task_id | assigned_agent | inputs | dependencies | definition_of_done |
|---|---|---|---|---|
| MEEKO-001 | Architect | PRD §2–4, this doc | user answers Q1–Q4 | ERD/entity map, IPC + hookd socket contract (OpenAPI-ish), directory skeleton spec |
| MEEKO-002 | Git Operator | repo URL from user | MEEKO-001 | greenfield repo, GPL-3.0 LICENSE, branch conventions (feature/ fix/ chore/), Conventional Commits enforced |
| MEEKO-003 | Developer | Architect's skeleton | MEEKO-001, 002 | impasto base imported + renamed, all inherited verbs green |
| MEEKO-004 | Developer | hookd contract | MEEKO-003 | meeko-hookd with SO_PEERCRED auth'd approval channel + keyring bridge |
| MEEKO-005 | QA & Testing | IPC contracts | MEEKO-003 | unit + contract suites green locally, coverage ≥80% changed lines |
| MEEKO-006 | Code Review & Security | diff of 003–004 | MEEKO-005 | APPROVED on OWASP pass, secrets, approval-channel auth |
| MEEKO-007 | DevOps & CI/CD | repo + stack | MEEKO-002 | CI: qmllint + setup check + gitleaks + audit + tests, optional headless smoke |
