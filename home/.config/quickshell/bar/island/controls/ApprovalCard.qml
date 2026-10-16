// ╭──────────────────────────────────────────────────────────────────────────╮
// │                                                                          │
// │   A P P R O V A L   C A R D                                              │
// │   leaf control · renders one approval, wires respond/reject            │
// │                                                                          │
// │   Calls MeekoService.respondApproval (socket path, never qs ipc).       │
// │   Renders pending / resolved / error states. All host-sourced          │
//   │   strings pass through MeekoService.escapeDisplay once.               │
// │                                                                          │
// ╰──────────────────────────────────────────────────────────────────────────╯

import QtQuick
import QtQuick.Layouts
import Quickshell

import "../../../theme"
import "../../../services"
import "../../../components"

// A single approval card. Shows the question, option buttons, and a reject
// button. On respond, calls MeekoService.respondApproval — the verdict goes
// through hookd's socket, never the QML IPC layer (CR&S constraint 1).
ColumnLayout {
    id: root

    property var approval: null

    readonly property string entryId: root.approval?.entryId ?? ""
    readonly property string question: root.approval?.question ?? ""
    readonly property var options: root.approval?.options ?? []
    readonly property string agentId: root.approval?.agentId ?? ""

    // Pending = still in MeekoService.approvals. Resolved = gone (hookd
    // pushed approval.resolve after the host processed the verdict).
    readonly property bool pending: MeekoService.approvals.some(
        a => a.entryId === root.entryId)

    spacing: Theme.cardRowGap

    // ── QUESTION ─────────────────────────────────────────────────────────────

    Text {
        Layout.fillWidth: true
        text: MeekoService.escapeDisplay(root.question)
        color: Theme.text
        wrapMode: Text.Wrap
        maximumLineCount: 3
        elide: Text.ElideRight
        font.pixelSize: Theme.fontSizeRegular
    }

    // ── OPTION BUTTONS ───────────────────────────────────────────────────────

    RowLayout {
        Layout.fillWidth: true
        spacing: 8

        Repeater {
            model: ScriptModel {
                values: root.options
                objectProp: "id"
            }

            PillButton {
                required property var modelData
                text: MeekoService.escapeDisplay(modelData.label || modelData.id)
                icon: modelData.id === "yes" ? "󰄬" : (modelData.id === "no" ? "󰅖" : "")
                onClicked: MeekoService.respondApproval(root.entryId, modelData.id)
            }
        }

        // Reject: respond with null optionId (host treats as default)
        PillButton {
            text: "Reject"
            icon: "󰅖"
            onClicked: MeekoService.respondApproval(root.entryId, null)
        }
    }

    // ── RESOLVED STATE ───────────────────────────────────────────────────────

    Text {
        visible: !root.pending
        text: "Resolved"
        color: Theme.textDim
        font.pixelSize: Theme.fontSizeSmall
        font.italic: true
    }
}
