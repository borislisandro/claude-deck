/*
 * The arrow that opens and folds a card or a row in the popup. It turns over (as
 * the island's chevron does) rather than swapping icons; only the arrow turns, not
 * the button's hover background.
 */
import QtQuick
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

PlasmaComponents3.ToolButton {
    id: button

    property bool open: false
    property string openText: i18n("Fold")
    property string closedText: i18n("Open")

    /// The arrow's angle: down when folded, up when open.
    property real turn: open ? 180 : 0
    Behavior on turn { NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }

    icon.name: "arrow-down-symbolic"
    display: PlasmaComponents3.AbstractButton.IconOnly
    text: open ? openText : closedText
    PlasmaComponents3.ToolTip.text: text
    PlasmaComponents3.ToolTip.visible: hovered
    Behavior on opacity { NumberAnimation { duration: Kirigami.Units.shortDuration } }

    // The stock content (the theme's icon, at the theme's size) turns; the background stays.
    Binding {
        target: button.contentItem
        property: "rotation"
        value: button.turn
    }
}
