/*
 * The popup's one entrance: its item starts a little off, faded out, and settles
 * into place. The cards use it when the popup opens; an opened-up session or tab
 * uses it for what it reveals. `play()` runs it. It takes Plasma's animation speed,
 * and with animations turned off it only makes sure the item is in place.
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
    property real distance: Kirigami.Units.gridUnit * 0.6

    function play() {
        anim.stop()
        if (!target) return
        if (Kirigami.Units.longDuration <= 1) {   // animations off (Kirigami then says 1 ms)
            rise.y = 0
            target.opacity = 1
            return
        }
        // Set here, not in the animation, so not one frame shows it at home first.
        rise.y = rise.distance
        target.opacity = 0
        anim.start()
    }

    property SequentialAnimation anim: SequentialAnimation {
        PauseAnimation { duration: rise.delay }
        ParallelAnimation {
            NumberAnimation {
                target: rise
                property: "y"
                to: 0
                duration: Kirigami.Units.longDuration
                easing.type: Easing.OutCubic
            }
            // Readable before it has quite settled.
            NumberAnimation {
                target: rise.target
                property: "opacity"
                to: 1
                duration: Math.round(Kirigami.Units.longDuration * 0.7)
                easing.type: Easing.OutQuad
            }
        }
    }
}
