/*
 * The status light. Working breathes; background work breathes slower and shallower
 * (busy, but not with you); needs-you pings (the light dips as a ripple leaves it);
 * an unseen your-turn sends a slow ripple; seen states sit still.
 * Every rhythm is a multiple of one 1.2 s beat, so lights side by side keep time
 * with each other and with the island's glow instead of drifting in and out.
 * Nothing loops while the light is off screen (a closed popup, a hidden tooltip)
 * or while animations are turned off in Plasma.
 */
import QtQuick
import org.kde.kirigami as Kirigami

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
    readonly property bool background: status === "background"
    readonly property bool calling: needsYou || waiting

    /// On screen, with animations on.
    readonly property bool live: visible && Kirigami.Units.longDuration > 1
        && Window.window !== null && Window.window.visible
    /// One ripple: a beat while something needs you, two once it has waited a minute
    /// (or for a your-turn, which is less pressing).
    readonly property int period: needsYou && waited <= 60000 ? 1200 : 2400
    /// Multiplies the core's opacity: what the breathing and the ping animate.
    property real pulse: 1

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
        visible: call.running
    }

    ParallelAnimation {
        id: call
        running: dot.calling && dot.live
        loops: Animation.Infinite
        NumberAnimation {
            target: ripple; property: "scale"; from: 1; to: 2.2
            duration: dot.period; easing.type: Easing.OutCubic
        }
        NumberAnimation {
            target: ripple; property: "opacity"; from: 0.85; to: 0
            duration: dot.period; easing.type: Easing.OutCubic
        }
        // The light dips as the ring leaves it (needs-you only; a your-turn glows steady).
        SequentialAnimation {
            NumberAnimation {
                target: dot; property: "pulse"; to: dot.needsYou ? 0.35 : 1
                duration: dot.period / 2; easing.type: Easing.InOutSine
            }
            NumberAnimation {
                target: dot; property: "pulse"; to: 1
                duration: dot.period / 2; easing.type: Easing.InOutSine
            }
        }
        onRunningChanged: if (!running) dot.pulse = 1
    }

    SequentialAnimation {
        id: breathe
        running: (dot.working || dot.background) && dot.live
        loops: Animation.Infinite
        NumberAnimation { target: dot; property: "pulse"; to: dot.background ? 0.6 : 0.45; duration: dot.background ? 2400 : 1200; easing.type: Easing.InOutSine }
        NumberAnimation { target: dot; property: "pulse"; to: 1; duration: dot.background ? 2400 : 1200; easing.type: Easing.InOutSine }
        onRunningChanged: if (!running) dot.pulse = 1
    }
    // Breathing from working to background (Claude ended its turn) starts a fresh breath.
    onBackgroundChanged: if (breathe.running) breathe.restart()

    // A new rhythm (a minute of waiting) starts on a fresh cycle, not mid-ring.
    onPeriodChanged: if (call.running) call.restart()

    // Soft glow behind the dot.
    Rectangle {
        anchors.centerIn: parent
        width: dot.size * 1.9
        height: width
        radius: width / 2
        color: dot.color
        opacity: dot.calling ? 0.22 : (dot.working ? 0.12 : (dot.background ? 0.08 : 0))
        Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }
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
        opacity: (dot.status === "your_turn" && dot.seen ? 0.85 : 1) * dot.pulse
        Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }
    }
}
