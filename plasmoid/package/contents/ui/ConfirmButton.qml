/*
 * A tool button that acts at once, or, when `needsConfirm`, asks inline first:
 * the first click turns it into a red "Close?" for a few seconds, the second acts.
 * Nothing leaves the popup, so it stays open for the next one.
 */
import QtQuick
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

PlasmaComponents3.ToolButton {
    id: button

    property bool needsConfirm: false
    property string confirmText: i18n("Close?")
    property string tip: ""
    property bool armed: false

    signal confirmed()

    icon.name: armed ? "dialog-warning-symbolic" : "window-close-symbolic"
    display: armed ? PlasmaComponents3.AbstractButton.TextBesideIcon : PlasmaComponents3.AbstractButton.IconOnly
    text: armed ? confirmText : tip
    icon.color: armed ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
    PlasmaComponents3.ToolTip.text: armed ? i18n("Click again to confirm") : tip
    PlasmaComponents3.ToolTip.visible: hovered && tip.length > 0
    // Cards show it faintly until hovered.
    Behavior on opacity { NumberAnimation { duration: Kirigami.Units.shortDuration } }

    onClicked: {
        if (!needsConfirm || armed) {
            armed = false
            confirmed()
        } else {
            armed = true
            disarm.restart()
        }
    }

    Timer {
        id: disarm
        interval: 3500
        onTriggered: button.armed = false
    }
}
