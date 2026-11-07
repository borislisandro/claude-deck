/*
 * The arrow that opens and folds a card or a row in the popup.
 */
import QtQuick
import org.kde.plasma.components as PlasmaComponents3

PlasmaComponents3.ToolButton {
    id: button

    property bool open: false
    property string openText: i18n("Fold")
    property string closedText: i18n("Open")

    icon.name: open ? "arrow-up-symbolic" : "arrow-down-symbolic"
    display: PlasmaComponents3.AbstractButton.IconOnly
    text: open ? openText : closedText
    PlasmaComponents3.ToolTip.text: text
    PlasmaComponents3.ToolTip.visible: hovered
}
