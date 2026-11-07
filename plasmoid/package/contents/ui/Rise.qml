/*
 * A transform that floats its item into place like a released balloon: it starts
 * a little off (toward the panel), faded out, and drifts in with a soft bob.
 * `play()` runs it; with animations turned off in Plasma it does nothing.
 */
import QtQuick
import org.kde.kirigami as Kirigami

Translate {
    id: rise

    /// The item to fade in (the one this transform is on).
    property Item target
    /// Waits this long (ms) before moving: set per item for a staggered entrance.
    property int delay: 0
    /// How far it starts from home, in px; negative starts above.
    property real distance: Kirigami.Units.gridUnit * 1.3

    function play() {
        if (!target || Kirigami.Units.longDuration === 0) return
        anim.restart()
    }

    property SequentialAnimation anim: SequentialAnimation {
        ScriptAction {
            script: {
                rise.y = rise.distance
                rise.target.opacity = 0
            }
        }
        PauseAnimation { duration: rise.delay }
        ParallelAnimation {
            NumberAnimation {
                target: rise
                property: "y"
                to: 0
                duration: 460
                easing.type: Easing.OutBack
                easing.overshoot: 2.4
            }
            NumberAnimation {
                target: rise.target
                property: "opacity"
                to: 1
                duration: 220
                easing.type: Easing.OutQuad
            }
        }
    }
}
