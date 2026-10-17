// ╭──────────────────────────────────────────────────────────────────────────╮
// │                                                                          │
// │   A G E N T   P A N E L                                                    │
// │   island · approval cards, agent status, usage chip                     │
// │                                                                          │
// │   Interactive view over MeekoService. Layout owner, not state owner.   │
// │   No Timer polling — re-renders on SSE mirror updates only.              │
// │                                                                          │
// ╰──────────────────────────────────────────────────────────────────────────╯

import QtQuick
import QtQuick.Layouts
import Quickshell

import "../../theme"
import "../../services"
import "../../components"

// The agent surface panel: pending approval cards, agent roster, and provider
// usage. Reads everything from MeekoService — no local mutation (CR&S
// constraint 4). Re-renders are driven by the SSE mirror, never a Timer.
ColumnLayout {
    id: root

    signal closed()

    spacing: Theme.cardRowGap

    // ── HEADER ───────────────────────────────────────────────────────────────

    RowLayout {
        Layout.fillWidth: true
        Layout.preferredHeight: Theme.cardRow

        Text {
            text: "Agents"
            font.family: Theme.fontFamily
            font.pixelSize: Theme.fontSizeLarge
            font.weight: Font.DemiBold
            color: Theme.text
            Layout.fillWidth: true
        }

        // Link state indicator
        Text {
            text: MeekoService.linkState
            color: MeekoService.linkState === "online" ? Theme.indicator : Theme.red
            font.pixelSize: Theme.fontSizeSmall
            font.weight: Font.Medium
        }

        IconButton {
            icon: "󰅖"
            onClicked: root.closed()
        }
    }

    // ── APPROVAL CARDS ───────────────────────────────────────────────────────

    Repeater {
        model: ScriptModel {
            values: MeekoService.approvals
            objectProp: "entryId"
        }

        ApprovalCard {
            required property var modelData
            Layout.fillWidth: true
            approval: modelData
        }
    }

    // Empty state when no pending approvals
    Text {
        visible: MeekoService.approvals.length === 0
        text: "No pending approvals"
        color: Theme.textDim
        font.pixelSize: Theme.fontSizeRegular
        Layout.fillWidth: true
        Layout.preferredHeight: Theme.cardRow
        verticalAlignment: Text.AlignVCenter
    }

    // ── AGENT ROSTER ─────────────────────────────────────────────────────────

    RowLayout {
        Layout.fillWidth: true
        Layout.preferredHeight: Theme.cardRow

        Text {
            text: "Agents"
            color: Theme.textDim
            font.pixelSize: Theme.fontSizeSmall
            font.weight: Font.Medium
            Layout.fillWidth: true
        }

        Text {
            text: `${MeekoService.agents.length}`
            color: Theme.text
            font.pixelSize: Theme.fontSizeSmall
        }
    }

    Repeater {
        model: ScriptModel {
            values: MeekoService.agents.slice(0, 5)
            objectProp: "id"
        }

        RowLayout {
            required property var modelData

            Layout.fillWidth: true
            Layout.preferredHeight: Theme.cardRow

            Text {
                text: modelData.name || modelData.id
                color: Theme.text
                Layout.fillWidth: true
                elide: Text.ElideRight
                font.pixelSize: Theme.fontSizeRegular
            }

            Text {
                text: modelData.status || ""
                color: Theme.textDim
                font.pixelSize: Theme.fontSizeSmall
            }
        }
    }

    // ── USAGE CHIP ───────────────────────────────────────────────────────────

    RowLayout {
        Layout.fillWidth: true
        Layout.preferredHeight: Theme.cardRow

        Text {
            text: "Usage"
            color: Theme.textDim
            font.pixelSize: Theme.fontSizeSmall
            font.weight: Font.Medium
            Layout.fillWidth: true
        }
    }

    Repeater {
        model: ScriptModel {
            values: MeekoService.usage
            objectProp: "provider"
        }

        RowLayout {
            required property var modelData

            Layout.fillWidth: true
            Layout.preferredHeight: Theme.cardRow

            Text {
                text: modelData.provider || ""
                color: Theme.text
                Layout.fillWidth: true
                font.pixelSize: Theme.fontSizeRegular
            }

            UsageBar {
                progress: modelData.limit > 0 ? Math.min(1, modelData.used / modelData.limit) : 0
            }
        }
    }
}
