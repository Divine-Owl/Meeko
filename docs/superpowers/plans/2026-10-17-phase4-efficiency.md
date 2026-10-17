# Phase 4 Efficiency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Meeko's resident footprint inside the 500 MB never-exceed line while preserving impasto's visual identity.

**Architecture:** ShellModeService singleton drives quiet-mode state; HyprlandService pushes fullscreen events to trigger transitions; lazy-loaded modules use Loader with prewarm; systemd units enforce memory caps. Push-not-poll throughout.

**Tech Stack:** Quickshell QML, systemd user units, bash setup script

**Spec:** `docs/superpowers/specs/2026-10-17-meeko-phase4-efficiency-design.md`

## Global Constraints

- License: GPL-3.0-only (impasto base)
- Memory budget: quickshell 430M max / 350M high, hookd 64M max; total ≤ 500 MB
- Push-not-poll: no Timer polling anywhere in Phase 4
- Quiet mode suspends animation/GPU surfaces, never interaction-driven motion
- Setup: root changes via pkexec, never terminal password prompt
- Swap config: detect existing, respect, never overwrite, support dry-run

## Review Focus

- Quiet-mode entry/exit: does the island freeze without visual artifacts?
- Coalesced repaint on quiet exit: one frame, no catch-up burst?
- systemd caps: does the unit file correctly import session env?
- Setup dry-run: does it detect existing swap without overwriting?
- repaintCount: monotonic, testable seam for QA

---

### Task 1: ShellModeService singleton

**Files:**
- Create: `home/.config/quickshell/services/ShellModeService.qml`
- Modify: `home/.config/quickshell/qmldir`

**Interfaces:**
- Consumes: HyprlandService (fullscreen events), SettingsService (manual override)
- Produces: `ShellModeService.mode` (string: "active"|"quiet"), `.source` (string), `.repaintCount` (int)

- [ ] **Step 1: Create ShellModeService.qml**

```qml
pragma Singleton
import QtQuick
import Quickshell
import "../theme"

Singleton {
    id: root
    property string mode: "active"
    property string source: "auto:fullscreen"
    property int repaintCount: 0

    // Watch Hyprland fullscreen events
    // On fullscreen=true → setQuiet("auto:fullscreen")
    // On fullscreen=false → setActive("auto:fullscreen")
    // Manual toggle via control centre
    // repaintCount increments on every transition
}
```

- [ ] **Step 2: Register in qmldir**

Add `singleton ShellModeService 1.0 services/ShellModeService.qml` to qmldir.

- [ ] **Step 3: Verify it loads**

Run: `qmllint home/.config/quickshell/services/ShellModeService.qml` (Qt6 linter)
Expected: exit 0

- [ ] **Step 4: Commit**

```bash
git add home/.config/quickshell/services/ShellModeService.qml home/.config/quickshell/qmldir
git commit -m "feat(efficiency): add ShellModeService singleton"
```

---

### Task 2: Quiet mode integration

**Files:**
- Modify: `home/.config/quickshell/bar/widgets/DynamicIsland.qml`
- Modify: `home/.config/quickshell/bar/island/IslandState.qml`

**Interfaces:**
- Consumes: `ShellModeService.mode`, `.repaintCount`
- Produces: visual suspend/resume on island surfaces

- [ ] **Step 1: Wire ShellModeService to DynamicIsland**

Add Connections to ShellModeService; on mode change, suspend/resume animation drivers (spectrum shader, sheen/blur). Bar stays static. Hookd socket stays live.

- [ ] **Step 2: Coalesced repaint on exit**

On transition to active, apply one coalesced frame. No catch-up burst.

- [ ] **Step 3: Verify visually**

Manual test: trigger fullscreen, confirm island freezes; exit, confirm one clean frame.

- [ ] **Step 4: Commit**

```bash
git add home/.config/quickshell/bar/widgets/DynamicIsland.qml
git commit -m "feat(efficiency): wire quiet mode to DynamicIsland"
```

---

### Task 3: Tiering / lazy load

**Files:**
- Modify: `home/.config/quickshell/services/ModuleService.qml`

**Interfaces:**
- Consumes: ModuleService.catalogue, SettingsService
- Produces: lazy-load trigger per module

- [ ] **Step 1: Add load triggers**

For each opt-in module, add a prewarm trigger (~150 ms before reveal). Use Loader with asynchronous: true.

- [ ] **Step 2: Verify first-open content**

Open an opt-in panel, assert content on first frame.

- [ ] **Step 3: Commit**

```bash
git add home/.config/quickshell/services/ModuleService.qml
git commit -m "feat(efficiency): add lazy-load tiering for opt-in modules"
```

---

### Task 4: systemd units + caps

**Files:**
- Create: `system/etc/systemd/user/quickshell.service`
- Create: `system/etc/systemd/user/meeko-hookd.service`
- Modify: `setup` (add unit installation step)

**Interfaces:**
- Consumes: nothing (leaf config files)
- Produces: systemd user units with MemoryHigh/MemoryMax caps

- [ ] **Step 1: Create quickshell.service**

```ini
[Unit]
Description=Meeko shell
After=graphical-session.target

[Service]
ExecStart=/usr/bin/qs
Restart=on-failure
RestartSec=1s
MemoryHigh=350M
MemoryMax=430M

[Install]
WantedBy=graphical-session.target
```

- [ ] **Step 2: Create meeko-hookd.service**

```ini
[Unit]
Description=Meeko hookd
After=graphical-session.target

[Service]
ExecStart=/usr/local/bin/meeko-hookd
Restart=on-failure
RestartSec=1s
MemoryMax=64M

[Install]
WantedBy=graphical-session.target
```

- [ ] **Step 3: Add to setup script**

Install units via `systemctl --user enable`. Import session env via ImportEnvironment or env file.

- [ ] **Step 4: Verify**

`systemd-analyze verify system/etc/systemd/user/quickshell.service` — no errors.

- [ ] **Step 5: Commit**

```bash
git add system/etc/user/ setup
git commit -m "feat(efficiency): add systemd units with memory caps"
```

---

### Task 5: Setup script

**Files:**
- Modify: `setup`

**Interfaces:**
- Consumes: nothing
- Produces: first-run setup — packages, swap/zram, session env file, welcome flag

- [ ] **Step 1: Add swap detection**

Detect existing swap/hibernation. Respect it, never overwrite. Dry-run support.

- [ ] **Step 2: Add session env file generation**

Write env file with WAYLAND_DISPLAY, HYPRLAND_INSTANCE_SIGNATURE, XDG_RUNTIME_DIR.

- [ ] **Step 3: Add first-run flag**

Write a flag file for the in-shell welcome panel.

- [ ] **Step 4: Verify**

Run `setup --dry-run` — should detect existing swap without modifying it.

- [ ] **Step 5: Commit**

```bash
git add setup
git commit -m "feat(efficiency): add setup script with swap detection and session env"
```
