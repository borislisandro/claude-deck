/*
 * One session on the island: spark + status light (+ wait time).
 * Hover peeks at its pane and shows a card; click jumps to it.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

PlasmaCore.ToolTipArea {
    id: item

    required property var session
    required property var deckRoot
    property real glyphSize: 16

    readonly property string status: session.status
    readonly property bool seen: session.seen
    readonly property color tint: deckRoot.statusColor(status, seen)
    readonly property real waited: deckRoot.now - session.since
    readonly property bool showTime: Plasmoid.configuration.showElapsed
        && (status === "needs_you" || status === "your_turn") && !seen && waited >= 60000
    readonly property bool isFocused: deckRoot.focusedSid === session.sid

    implicitWidth: row.implicitWidth + Kirigami.Units.smallSpacing
    implicitHeight: row.implicitHeight

    mainItem: HoverCard {
        session: item.session
        deckRoot: item.deckRoot
    }
    location: Plasmoid.location
    interactive: false

    onContainsMouseChanged: {
        if (containsMouse) {
            deckRoot.fetchTail(session.sid, 8)
            peekDelay.restart()
        } else {
            peekDelay.stop()
            deckRoot.requestUnpeek()
        }
    }

    Timer {
        id: peekDelay
        interval: 320
        onTriggered: item.deckRoot.requestPeek(item.session.sid)
    }

    // The focused session gets a faint underline, like an active task.
    Rectangle {
        anchors.horizontalCenter: row.horizontalCenter
        anchors.top: row.bottom
        anchors.topMargin: 1
        width: row.width * 0.7
        height: 2
        radius: 1
        color: item.tint
        opacity: item.isFocused && item.deckRoot.deckActive ? 0.8 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }
    }

    RowLayout {
        id: row
        anchors.centerIn: parent
        spacing: 1

        Item {
            Layout.preferredWidth: item.glyphSize
            Layout.preferredHeight: item.glyphSize
            Spark {
                anchors.centerIn: parent
                size: item.glyphSize * 0.92
                spinning: item.status === "working" && Plasmoid.configuration.spinWhileWorking
                color: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                opacity: item.status === "exited" ? 0.45 : 1
            }
            // A small "!" when the turn ended on an API error.
            Rectangle {
                visible: !!item.session.error
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.rightMargin: -2
                anchors.topMargin: -2
                width: 8; height: 8; radius: 4
                color: Plasmoid.configuration.colorYourTurn
                PlasmaComponents3.Label {
                    anchors.centerIn: parent
                    text: "!"
                    font.pixelSize: 7
                    font.bold: true
                    color: "white"
                }
            }
        }

        StatusDot {
            Layout.alignment: Qt.AlignVCenter
            size: Math.round(item.glyphSize * 0.5)
            status: item.status
            seen: item.seen
            color: item.tint
            waited: item.waited
        }

        PlasmaComponents3.Label {
            visible: item.showTime
            text: item.deckRoot.elapsed(item.session.since)
            color: item.tint
            font.pixelSize: Math.round(item.glyphSize * 0.68)
            font.weight: Font.DemiBold
            font.features: { "tnum": 1 }
            Layout.alignment: Qt.AlignVCenter
            Layout.rightMargin: 2
        }
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton
        cursorShape: Qt.PointingHandCursor
        onClicked: item.deckRoot.activate(item.session.sid)
    }
}
