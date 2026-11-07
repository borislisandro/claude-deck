/*
 * The island: a critter + light per session. With more sessions than
 * "collapseAfter", one critter per deck tab (its most urgent session, with a
 * count); with more tabs than that too, a count per state.
 * Zero width while the deck isn't running.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Item {
    id: compact

    required property var deckRoot

    readonly property bool running: deckRoot.running
    readonly property var sessions: deckRoot.sessions
    readonly property var groups: deckRoot.groups
    /// "sessions" | "tabs" | "counts"
    readonly property string mode: sessions.length <= Plasmoid.configuration.collapseAfter ? "sessions"
        : groups.length <= Plasmoid.configuration.collapseAfter ? "tabs" : "counts"
    readonly property bool collapsed: mode === "counts"
    /// Critter pixel size: whole pixels, as large as the pill allows.
    readonly property int px: Math.max(2, Math.floor(height * 0.84 / 11))
    readonly property real glyph: px * 8
    readonly property real pad: Math.round(px * 4)
    readonly property color attention: deckRoot.unseen > 0
        ? (deckRoot.deck.counts && deckRoot.deck.counts.needs_you > 0 ? Plasmoid.configuration.colorNeedsYou : Plasmoid.configuration.colorYourTurn)
        : "transparent"

    visible: running
    property real wantedWidth: running ? pill.width + Kirigami.Units.smallSpacing : 0
    Layout.minimumWidth: wantedWidth
    Layout.preferredWidth: wantedWidth
    Layout.maximumWidth: wantedWidth
    Layout.fillHeight: true

    Behavior on wantedWidth {
        NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic }
    }

    // Background clicks: left opens the popup, middle starts a session, wheel switches.
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        onClicked: mouse => {
            if (mouse.button === Qt.MiddleButton) {
                compact.deckRoot.run("new --pick")
            } else {
                compact.deckRoot.expanded = !compact.deckRoot.expanded
            }
        }
        property real wheelAcc: 0
        onWheel: wheel => {
            wheelAcc += wheel.angleDelta.y
            if (Math.abs(wheelAcc) >= 120) {
                compact.deckRoot.cycle(wheelAcc > 0 ? -1 : 1)
                wheelAcc = 0
            }
        }
    }

    Rectangle {
        id: pill
        anchors.centerIn: parent
        height: Math.min(compact.height, Math.max(Math.round(compact.height * 0.74), compact.px * 11 + 4))
        width: content.implicitWidth + compact.pad * 2
        radius: height / 2
        color: Plasmoid.configuration.ownBackground
            ? Qt.rgba(Kirigami.Theme.backgroundColor.r, Kirigami.Theme.backgroundColor.g, Kirigami.Theme.backgroundColor.b, 0.35)
            : "transparent"
        border.width: compact.deckRoot.unseen > 0 ? 1 : 0
        border.color: compact.deckRoot.unseen > 0
            ? Qt.rgba(compact.attention.r, compact.attention.g, compact.attention.b, 0.75)
            : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.10)
        Behavior on border.color { ColorAnimation { duration: 300 } }
        Behavior on width { NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }

        // A slow glow across the pill while anything waits unseen.
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: compact.attention
            opacity: 0
            visible: compact.deckRoot.unseen > 0
            SequentialAnimation on opacity {
                running: compact.deckRoot.unseen > 0
                loops: Animation.Infinite
                NumberAnimation { from: 0; to: 0.16; duration: 1400; easing.type: Easing.InOutSine }
                NumberAnimation { from: 0.16; to: 0; duration: 1400; easing.type: Easing.InOutSine }
            }
        }

        RowLayout {
            id: content
            anchors.centerIn: parent
            spacing: compact.px * 3

            // Deck running but no sessions yet (picker open), or collapsed: one critter.
            Critter {
                visible: compact.sessions.length === 0 || compact.collapsed
                px: compact.px
                mood: compact.sessions.length === 0 ? "idle"
                    : ((compact.deckRoot.deck.counts || {}).working > 0 ? "running" : "resting")
                body: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                animated: Plasmoid.configuration.spinWhileWorking
                Layout.alignment: Qt.AlignVCenter
            }

            Repeater {
                model: compact.mode === "sessions" ? compact.sessions : []
                delegate: SessionIcon {
                    required property var modelData
                    session: modelData
                    deckRoot: compact.deckRoot
                    px: compact.px
                    Layout.alignment: Qt.AlignVCenter
                }
            }

            // One critter per tab: the tab's most urgent session, and how many it holds.
            Repeater {
                model: compact.mode === "tabs" ? compact.groups : []
                delegate: SessionIcon {
                    required property var modelData
                    session: modelData.top
                    groupSessions: modelData.sessions
                    tabIndex: modelData.tabIndex
                    deckRoot: compact.deckRoot
                    px: compact.px
                    Layout.alignment: Qt.AlignVCenter
                }
            }

            // Collapsed: one count per state, most urgent first.
            Repeater {
                model: compact.collapsed ? [
                    { key: "needs_you", seen: false },
                    { key: "your_turn", seen: false },
                    { key: "working", seen: true },
                    { key: "exited", seen: true }
                ] : []
                delegate: RowLayout {
                    required property var modelData
                    readonly property int n: (compact.deckRoot.deck.counts || {})[modelData.key] || 0
                    visible: n > 0
                    spacing: 2
                    StatusDot {
                        size: Math.round(compact.glyph * 0.5)
                        status: modelData.key
                        seen: modelData.key !== "your_turn" || compact.deckRoot.unseen === 0
                        color: compact.deckRoot.statusColor(modelData.key, seen)
                    }
                    PlasmaComponents3.Label {
                        text: n
                        font.weight: Font.DemiBold
                        font.pixelSize: Math.round(compact.glyph * 0.75)
                    }
                }
            }
        }
    }
}
