/*
 * The popup: a live thumbnail of the deck, then the sessions grouped the way the
 * deck has them: a card per tab (one session: the full card; several: a header
 * and a line per session).
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.kirigami as Kirigami
import org.kde.taskmanager as TaskManager
import org.kde.pipewire as PipeWire

PlasmaExtras.Representation {
    id: full

    required property var deckRoot

    Layout.minimumWidth: Kirigami.Units.gridUnit * 26
    Layout.preferredWidth: Kirigami.Units.gridUnit * 30
    // Plasma opens popups at their minimum size, so the minimum carries the real height.
    Layout.minimumHeight: Kirigami.Units.gridUnit * full.deckRoot.popupUnits
    // Plasma sizes the popup before its content exists, so size it from what it will show.
    Layout.preferredHeight: Kirigami.Units.gridUnit * full.deckRoot.popupUnits
    collapseMarginsHint: true

    header: PlasmaExtras.PlasmoidHeading {
        RowLayout {
            anchors.fill: parent
            spacing: Kirigami.Units.smallSpacing
            Critter {
                px: 2
                mood: (full.deckRoot.deck.counts || {}).working > 0 ? "running" : "idle"
                body: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                Layout.leftMargin: Kirigami.Units.smallSpacing
            }
            PlasmaExtras.Heading {
                level: 3
                text: "Claude Deck"
            }
            PlasmaComponents3.Label {
                text: full.deckRoot.running ? full.deckRoot.deck.summary : i18n("not running")
                opacity: 0.65
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            PlasmaComponents3.ToolButton {
                icon.name: "list-add-symbolic"
                onClicked: full.deckRoot.run(full.deckRoot.running ? "new --pick" : "toggle")
                PlasmaComponents3.ToolTip.text: i18n("New session (middle-click the island)")
                PlasmaComponents3.ToolTip.visible: hovered
            }
            PlasmaComponents3.ToolButton {
                icon.name: "view-grid-symbolic"
                enabled: full.deckRoot.sessions.length > 1
                onClicked: full.deckRoot.run("tile")
                PlasmaComponents3.ToolTip.text: i18n("Tile all sessions side by side")
                PlasmaComponents3.ToolTip.visible: hovered
            }
            PlasmaComponents3.ToolButton {
                icon.name: "window-symbolic"
                onClicked: full.deckRoot.run("toggle")
                PlasmaComponents3.ToolTip.text: i18n("Show / hide the deck")
                PlasmaComponents3.ToolTip.visible: hovered
            }
        }
    }

    PlasmaComponents3.ScrollView {
        anchors.fill: parent
        PlasmaComponents3.ScrollBar.horizontal.policy: PlasmaComponents3.ScrollBar.AlwaysOff

        ColumnLayout {
            id: body
            width: full.width - Kirigami.Units.largeSpacing * 2
            x: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            // Live picture of the deck window.
            Rectangle {
                id: thumbFrame
                visible: Plasmoid.configuration.showThumbnail && full.deckRoot.deckUuid.length > 0 && thumb.nodeId > 0
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.smallSpacing
                Layout.preferredHeight: Math.min(width * 9 / 16, Kirigami.Units.gridUnit * 10)
                radius: Kirigami.Units.cornerRadius * 2
                color: Qt.rgba(0, 0, 0, 0.3)
                clip: true

                TaskManager.ScreencastingRequest {
                    id: cast
                    uuid: full.deckRoot.expanded && Plasmoid.configuration.showThumbnail ? full.deckRoot.deckUuid : ""
                }
                PipeWire.PipeWireSourceItem {
                    id: thumb
                    anchors.fill: parent
                    anchors.margins: 2
                    nodeId: cast.nodeId
                    visible: nodeId > 0
                }
                TapHandler {
                    onTapped: full.deckRoot.run("toggle")
                }
            }

            PlasmaExtras.PlaceholderMessage {
                visible: !full.deckRoot.running || full.deckRoot.sessions.length === 0
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.gridUnit * 2
                iconName: "utilities-terminal"
                text: full.deckRoot.running ? i18n("No sessions yet") : i18n("Claude Deck is not running")
                explanation: i18n("Meta+C opens the deck · Meta+Shift+C jumps to whatever is waiting")
            }

            Repeater {
                model: full.deckRoot.groups
                delegate: Loader {
                    id: groupLoader
                    required property var modelData
                    Layout.fillWidth: true
                    sourceComponent: modelData.sessions.length === 1 ? singleCard : groupCard
                    Component {
                        id: singleCard
                        SessionCard {
                            session: groupLoader.modelData.sessions[0]
                            deckRoot: full.deckRoot
                        }
                    }
                    Component {
                        id: groupCard
                        GroupCard {
                            group: groupLoader.modelData
                            deckRoot: full.deckRoot
                        }
                    }
                }
            }

            PlasmaComponents3.Label {
                visible: full.deckRoot.sessions.length > 0
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.smallSpacing
                Layout.bottomMargin: Kirigami.Units.smallSpacing
                horizontalAlignment: Text.AlignHCenter
                text: i18n("In the deck: Ctrl+Shift+Space next waiting · Ctrl+1…9 sessions · Ctrl+Shift+O overview")
                opacity: 0.5
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
        }
    }
}
