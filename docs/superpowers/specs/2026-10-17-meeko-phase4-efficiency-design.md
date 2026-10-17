# Meeko — Phase 4 Efficiency Design

Date: 2026-10-17 · Status: draft · Task: MEEKO-006

## 1. Purpose

Keep Meeko's resident footprint inside the 500 MB never-exceed line while preserving impasto's animations, palette-from-wallpaper, and shape-shifting island. Efficiency rules kill *waste* (idle timers, always-on shaders, unnecessary repaints), never *motion*. Push-not-poll throughout — no `Timer` polling anywhere in Phase 4.

## 2. ShellModeService (new singleton)

```
┌─────────────────────────────────────┐
│ ShellModeService                    │
│ mode: "active" | "quiet"            │
│ source: "auto:fullscreen" | "manual"│
│ repaintCount: int (monotonic)       │
└─────────────────────────────────────┘
```

- **Resolution:** manual override if set, else fullscreen ? quiet : active.
- **Triggers (push, never poll):** Hyprland fullscreen event via HyprlandService; manual toggle in the control centre.
- **repaintCount:** monotonic counter bumped by suspending surfaces — QA's testable seam, cheap now / awkward later.
- **Quiet suspends:** animation drivers (island/launcher/cards freeze mid-motion), GPU surfaces unmounted (spectrum shader, sheen/blur), non-essential panels stop repainting (bar stays, static), MeekoService keeps the socket live (projection current; at most one coalesced repaint on exit).
- **Quiet never suspends:** interaction-driven motion while active, the notification queue, hookd itself.
- **Exit quiet:** one coalesced frame applies latest state; full visual fidelity restored; no catch-up burst.

## 3. Tiering / lazy load

- **Core (always resident):** bar, island, launcher, dock, notifications, controls, settings, AgentModule, AgentPanel, ShellModeService, MeekoService, lock screen.
- **Default-on (cheap, push-driven, no timers):** clock, weather, battery, network, volume/brightness, media, stats, updates.
- **Opt-in (lazy-loaded on enable; zero idle cost otherwise):** desktop widget engines (Analogue/Sticker), capture surface, launcher extras, SDDM theme.
- **Per-module load triggers** are listed in the spec so QA can assert "opened panel shows content on first frame".
- **Prewarm on intent** (hover/focus/keybind/panel-open) ~150 ms before reveal so first paint has content — no pop-in, no layout shift.
- Once loaded, a module stays resident for the session (first-open cost paid once). Un-mount only in quiet mode or memory pressure, where a re-mount hitch is acceptable.

## 4. Systemd + caps

| Process | MemoryHigh | MemoryMax | Restart |
|---|---|---|---|
| quickshell (user unit) | 350M | 430M | on-failure, 1 s |
| meeko-hookd (user unit) | — | 64M | on-failure, 1 s |

- Hard-cap arithmetic: 430M + 64M = **494M < 500M**.
- **Session environment:** units bind to `graphical-session.target`, import `WAYLAND_DISPLAY`, `HYPRLAND_INSTANCE_SIGNATURE`, `XDG_RUNTIME_DIR` via `ImportEnvironment` or an env file generated at `meeko setup`.
- **hookd has no ordering dependency** on `codync-host.service`.
- **Budget authority:** the never-exceed line is the systemd cap sum — **500 MB total** for Meeko's own processes, with Codync's processes excluded.
- Pass/fail idle gate: total-process RSS flat over 10 min; cgroup-limited run degrades, never crash-loops.

## 5. Setup

- **Packages:** detect existing swap/hibernation configuration and respect it — never overwrite. Support dry-run.
- **Swap/zram policy:** recommended but skippable option on low-RAM machines. Memory config is informed and user-chosen, not forced. Root changes go through graphical authorization (pkexec), never a terminal password prompt.
- **Session env file:** generated at first run, imported by the quickshell unit.
- **First-run flag:** in-shell welcome panel vs CLI `setup` walkthrough (proposed: CLI `meeko setup` + in-shell "welcome" state flag for v1).

## 6. Out of scope

- Embedding or forking the Codync Electron host.
- Codync screens, voice, marketplace, routines, Android/iOS companions.
- Any change to the Codync host itself.
