// ╭──────────────────────────────────────────────────────────────────────────╮
// │                                                                          │
// │   S H E L L   M O D E   S E R V I C E                                    │
// │   active | quiet state · push-driven, never polled                      │
// │                                                                          │
// │   Owns the shell's quiet-mode state machine. HyprlandService pushes     │
// │   fullscreen events; a manual toggle overrides. repaintCount is the     │
// │   QA-testable seam for transition counting.                           │
// │                                                                          │
// ╰──────────────────────────────────────────────────────────────────────────╯

pragma Singleton

import QtQuick
import Quickshell

Singleton {
    id: root

    // ── STATE ───────────────────────────────────────────────────────────────

    property string mode: "active"
    property string source: "auto:fullscreen"
    property int repaintCount: 0

    // ── MANUAL TOGGLE ───────────────────────────────────────────────────────

    function toggle(): void {
        if (root.mode === "active")
            root.setQuiet("manual")
        else
            root.setActive("manual")
    }

    function setQuiet(src): void {
        if (root.mode === "quiet")
            return
        root.mode = "quiet"
        root.source = src
        root.repaintCount++
    }

    function setActive(src): void {
        if (root.mode === "active")
            return
        root.mode = "active"
        root.source = src
        root.repaintCount++
    }

    // ── FULLSCREEN EVENTS (push from HyprlandService) ───────────────────────

    readonly property Connections fullscreenWatch: Connections {
        target: HyprlandService

        function onFullscreenChanged(): void {
            if (HyprlandService.fullscreen && root.mode === "active")
                root.setQuiet("auto:fullscreen")
            else if (!HyprlandService.fullscreen && root.mode === "quiet" && root.source === "auto:fullscreen")
                root.setActive("auto:fullscreen")
        }
    }
}
