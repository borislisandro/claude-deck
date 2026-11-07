/*
 * One session on the island: its critter (running / thinking / resting) with
 * the status light centered beside its body, plus the wait time once it's long.
 * Hover peeks at its pane and shows a card; click jumps to it.
 */
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

PlasmaCore.ToolTipArea {
    id: item

    required property var session
    required property var deckRoot
    /// Critter pixel size, chosen by the island from the panel height.
    property int px: 2

    readonly property string status: session.status
    readonly property bool seen: session.seen
    readonly property color tint: deckRoot.statusColor(status, seen)
    readonly property real waited: deckRoot.now - session.since
    readonly property bool showTime: Plasmoid.configuration.showElapsed
        && (status === "needs_you" || status === "your_turn") && !seen && waited >= 60000
    readonly property bool isFocused: deckRoot.focusedSid === session.sid

    implicitWidth: critter.width + (showTime ? timeLabel.implicitWidth + 3 : 0) + 2
    implicitHeight: critter.height

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

    Critter {
        id: critter
        anchors.verticalCenter: parent.verticalCenter
        px: item.px
        mood: item.deckRoot.moodFor(item.status)
        accent: item.tint
        body: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
        animated: Plasmoid.configuration.spinWhileWorking
    }

    // The status light sits in the room to the critter's right, level with its body.
    StatusDot {
        id: light
        size: Math.max(6, Math.round(item.px * 3.4))
        x: critter.x + 14.3 * item.px - width / 2
        y: critter.y + critter.bodyCenterY - height / 2
        status: item.status
        seen: item.seen
        color: item.tint
        waited: item.waited
        Behavior on y { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
    }

    // A small "!" when the turn ended on an API error.
    Rectangle {
        visible: !!item.session.error
        x: critter.x
        y: critter.y
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

    PlasmaComponents3.Label {
        id: timeLabel
        visible: item.showTime
        x: critter.x + critter.width + 2
        anchors.verticalCenter: light.verticalCenter
        text: item.deckRoot.elapsed(item.session.since)
        color: item.tint
        font.pixelSize: Math.max(9, Math.round(item.px * 5))
        font.weight: Font.DemiBold
        font.features: { "tnum": 1 }
    }

    // The focused session gets a faint underline, like an active task.
    Rectangle {
        x: critter.x + critter.px
        y: critter.y + critter.height + 1
        width: critter.width - critter.px * 2
        height: 2
        radius: 1
        color: item.tint
        opacity: item.isFocused && item.deckRoot.deckActive ? 0.8 : 0
        Behavior on opacity { NumberAnimation { duration: 200 } }
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton
        cursorShape: Qt.PointingHandCursor
        onClicked: item.deckRoot.activate(item.session.sid)
    }
}
