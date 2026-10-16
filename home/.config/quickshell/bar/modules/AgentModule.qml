// ╭──────────────────────────────────────────────────────────────────────────╮
// │                                                                          │
// │   A G E N T   M O D U L E                                                │
// │   bar chip · thin status view over MeekoService                         │
// │                                                                          │
// │   Shows pending approval count and agent count. Click opens the         │
// │   AgentPanel in the island. Not a data owner — reads MeekoService.       │
// │                                                                          │
// ╰──────────────────────────────────────────────────────────────────────────╯

import QtQuick
import QtQuick.Layouts
import Quickshell

import "../../theme"
import "../../services"
import "../../components"

// The agent status chip on the bar. A thin view: it reads MeekoService and
// opens the panel on click. No local state beyond what the service owns.
Item {
    id: root

    implicitWidth: holder.implicitWidth
    implicitHeight: holder.implicitHeight

    readonly property int pendingCount: MeekoService.approvals.length
    readonly property int agentCount: MeekoService.agents.length
    readonly property bool online: MeekoService.linkState === "online"

    Loader {
        id: holder
        anchors.fill: parent
        sourceComponent: detail
    }

    function openPanel(): void {
        ModuleService.requestPanel("agent")
    }

    Component {
        id: detail

        ModuleCard {
            title: "Agents"
            subtitle: root.pendingCount > 0
                ? `${root.pendingCount} pending`
                : (root.online ? `${root.agentCount} agents` : "Offline")
            figure: root.pendingCount > 0 ? `${root.pendingCount}` : ""
            figureColor: root.pendingCount > 0 ? Theme.red : Theme.text

            mark: RingIndicator {
                anchors.fill: parent
                thickness: 3
                progress: 0
                trackColor: Theme.indicatorDim

                Text {
                    anchors.centerIn: parent
                    text: "󰧑"
                    font.family: Theme.fontMono
                    font.pixelSize: 20
                    color: root.pendingCount > 0 ? Theme.red : Theme.indicator
                }
            }

            // Pending approval previews (top 3)
            Repeater {
                model: ScriptModel {
                    values: MeekoService.approvals.slice(0, 3)
                    objectProp: "entryId"
                }

                Item {
                    required property var modelData

                    Layout.fillWidth: true
                    Layout.preferredHeight: Theme.cardRow

                    Text {
                        anchors.fill: parent
                        text: MeekoService.escapeDisplay(modelData.question || "")
                        elide: Text.ElideRight
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeSmall
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.preferredHeight: Theme.cardRow
                spacing: 8

                Item { Layout.fillWidth: true }

                PillButton {
                    implicitHeight: Theme.cardRow
                    text: "Open"
                    icon: "󰧑"
                    onClicked: root.openPanel()
                }
            }
        }
    }
}
