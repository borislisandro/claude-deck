/*
 * The background of a session card or row in the popup: a click opens the
 * session, a drag picks it up so it can be dropped beside another pane.
 * Sits under the card's buttons, so those still get their own clicks.
 */
import QtQuick

MouseArea {
    id: grip

    /// The popup's drag controller (FullRepresentation's `dnd`).
    property var dnd: null
    property var session: null
    signal tapped()

    property point start: Qt.point(0, 0)
    property bool dragging: false

    // Keeps the scroll view from taking the drag over as a flick.
    preventStealing: true
    cursorShape: dragging ? Qt.ClosedHandCursor : Qt.PointingHandCursor

    onPressed: m => {
        start = Qt.point(m.x, m.y)
        dragging = false
    }
    onPositionChanged: m => {
        if (!pressed || !dnd || !session || !session.kitty_window) return
        if (!dragging && Math.hypot(m.x - start.x, m.y - start.y) >= Qt.styleHints.startDragDistance) {
            dragging = true
            dnd.begin(session)
        }
        if (dragging) dnd.move(mapToItem(dnd.space, m.x, m.y))
    }
    onReleased: m => {
        if (dragging) {
            dragging = false
            dnd.drop()
        } else if (m.x >= 0 && m.y >= 0 && m.x <= width && m.y <= height) {
            tapped()
        }
    }
    onCanceled: {
        if (dragging) {
            dragging = false
            dnd.cancel()
        }
    }
}
