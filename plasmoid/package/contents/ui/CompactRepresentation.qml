/*
 * The island: a pill on the panel with one spark + light per session.
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
    readonly property bool collapsed: sessions.length > Plasmoid.configuration.collapseAfter
    readonly property real glyph: Math.max(12, Math.min(20, Math.round(height * 0.46)))
    readonly property real pad: Math.round(glyph * 0.55)
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
        height: Math.round(compact.height * 0.74)
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
            spacing: Math.round(compact.glyph * 0.35)

            // Deck running but no sessions yet (picker open): just the spark.
            Spark {
                visible: compact.sessions.length === 0 || compact.collapsed
                size: compact.glyph * 0.92
                color: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                spinning: compact.collapsed && compact.deckRoot.deck.counts && compact.deckRoot.deck.counts.working > 0
                    && Plasmoid.configuration.spinWhileWorking
            }

            Repeater {
                model: compact.collapsed ? [] : compact.sessions
                delegate: SessionIcon {
                    required property var modelData
                    session: modelData
                    deckRoot: compact.deckRoot
                    glyphSize: compact.glyph
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
