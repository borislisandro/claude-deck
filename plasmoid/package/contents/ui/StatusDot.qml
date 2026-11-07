/*
 * The status light. Working breathes, needs-you blinks with a quick ripple,
 * an unseen your-turn sends a slow ripple, seen states sit still.
 */
import QtQuick

Item {
    id: dot

    property string status: "exited"
    property bool seen: true
    property color color: "grey"
    property real size: 8
    /// Milliseconds spent in this state; long waits calm the animation down.
    property real waited: 0

    readonly property bool working: status === "working"
    readonly property bool needsYou: status === "needs_you"
    readonly property bool waiting: status === "your_turn" && !seen
    readonly property bool exited: status === "exited"

    width: size * 2.2
    height: size * 2.2

    // Expanding ring for states that want you.
    Rectangle {
        id: ripple
        anchors.centerIn: parent
        width: dot.size
        height: dot.size
        radius: width / 2
        color: "transparent"
        border.width: Math.max(1, dot.size * 0.18)
        border.color: dot.color
        opacity: 0
        visible: dot.needsYou || dot.waiting

        ParallelAnimation {
            running: ripple.visible
            loops: Animation.Infinite
            NumberAnimation {
                target: ripple; property: "scale"; from: 1; to: 2.2
                duration: dot.needsYou ? (dot.waited > 60000 ? 1800 : 950) : 2400
                easing.type: Easing.OutCubic
            }
            NumberAnimation {
                target: ripple; property: "opacity"; from: 0.85; to: 0
                duration: dot.needsYou ? (dot.waited > 60000 ? 1800 : 950) : 2400
                easing.type: Easing.OutCubic
            }
        }
    }

    // Soft glow behind the dot.
    Rectangle {
        anchors.centerIn: parent
        width: dot.size * 1.9
        height: width
        radius: width / 2
        color: dot.color
        opacity: dot.needsYou || dot.waiting ? 0.22 : (dot.working ? 0.12 : 0)
        Behavior on opacity { NumberAnimation { duration: 250 } }
    }

    Rectangle {
        id: core
        anchors.centerIn: parent
        width: dot.size
        height: dot.size
        radius: width / 2
        color: dot.exited ? "transparent" : dot.color
        border.width: dot.exited ? Math.max(1, dot.size * 0.2) : 0
        border.color: dot.color
        opacity: dot.status === "your_turn" && dot.seen ? 0.85 : 1
        Behavior on color { ColorAnimation { duration: 250 } }

        SequentialAnimation on opacity {
            id: breathe
            running: dot.working
            loops: Animation.Infinite
            NumberAnimation { from: 1; to: 0.4; duration: 850; easing.type: Easing.InOutSine }
            NumberAnimation { from: 0.4; to: 1; duration: 850; easing.type: Easing.InOutSine }
            onRunningChanged: if (!running) core.opacity = 1
        }

        SequentialAnimation on opacity {
            running: dot.needsYou
            loops: Animation.Infinite
            NumberAnimation { from: 1; to: 0.25; duration: dot.waited > 60000 ? 900 : 420; easing.type: Easing.InQuad }
            NumberAnimation { from: 0.25; to: 1; duration: dot.waited > 60000 ? 900 : 420; easing.type: Easing.OutQuad }
            onRunningChanged: if (!running) core.opacity = 1
        }
    }
}
