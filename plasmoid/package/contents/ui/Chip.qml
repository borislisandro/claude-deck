/*
 * A small rounded label: model, branch, counts.
 */
import QtQuick
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: chip

    property string text: ""
    property color tint: Kirigami.Theme.textColor
    property real strength: 0.08

    visible: text.length > 0
    radius: height / 2
    color: Qt.rgba(tint.r, tint.g, tint.b, strength)
    implicitWidth: label.implicitWidth + Kirigami.Units.largeSpacing
    implicitHeight: label.implicitHeight + 2

    PlasmaComponents3.Label {
        id: label
        anchors.centerIn: parent
        text: chip.text
        font: Kirigami.Theme.smallFont
        opacity: 0.85
    }
}
