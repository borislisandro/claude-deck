/*
 * One usage limit (5-hour or weekly) in the popup's header: how much of it is left,
 * big, a bar of what is left, and when it resets. Green while half or more is left,
 * yellow under half, red under a fifth (and once it is used up).
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami
import "logic.js" as Logic

Item {
    id: meter

    /// One entry of Logic.usageMeters.
    required property var info
    required property real now

    readonly property bool known: info.known === true
    // The theme has no yellow of its own (its "neutral" is orange), so pick one that
    // reads on the popup: Breeze's warm yellow on dark, a deeper amber on light.
    readonly property bool darkTheme: Kirigami.ColorUtils.brightnessForColor(Kirigami.Theme.backgroundColor) === Kirigami.ColorUtils.Dark
    readonly property color yellow: darkTheme ? "#fdbc4b" : "#b58500"
    readonly property color tint: !known ? Kirigami.Theme.disabledTextColor
        : info.level === "out" || info.level === "low" ? Kirigami.Theme.negativeTextColor
        : info.level === "half" ? yellow
        : Kirigami.Theme.positiveTextColor

    implicitHeight: col.implicitHeight + Kirigami.Units.smallSpacing * 2
    implicitWidth: col.implicitWidth + Kirigami.Units.largeSpacing * 2

    Rectangle {
        anchors.fill: parent
        radius: Kirigami.Units.cornerRadius * 2
        color: Qt.rgba(meter.tint.r, meter.tint.g, meter.tint.b, 0.08)
        border.width: 1
        border.color: Qt.rgba(meter.tint.r, meter.tint.g, meter.tint.b, 0.25)
        Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration } }
    }

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.largeSpacing
        anchors.rightMargin: Kirigami.Units.largeSpacing
        anchors.topMargin: Kirigami.Units.smallSpacing
        anchors.bottomMargin: Kirigami.Units.smallSpacing
        spacing: 2

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            PlasmaComponents3.Label {
                text: meter.info.label
                font: Kirigami.Theme.smallFont
                opacity: 0.7
            }
            Item { Layout.fillWidth: true }
            PlasmaComponents3.Label {
                text: !meter.known ? ""
                    : meter.info.resetsIn > 0 ? i18n("resets in %1", Logic.span(meter.info.resetsIn))
                    : i18n("fresh window")
                font: Kirigami.Theme.smallFont
                opacity: 0.7
                elide: Text.ElideLeft
                Layout.maximumWidth: col.width * 0.6
            }
        }

        RowLayout {
            spacing: Kirigami.Units.smallSpacing
            PlasmaComponents3.Label {
                text: meter.known ? meter.info.left + "%" : "—"
                color: meter.known ? meter.tint : Kirigami.Theme.textColor
                opacity: meter.known ? 1 : 0.5
                font.pixelSize: Math.round(Kirigami.Theme.defaultFont.pixelSize * 1.7)
                font.weight: Font.Bold
                Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration } }
            }
            PlasmaComponents3.Label {
                text: !meter.known ? i18n("after Claude's next reply") : meter.info.level === "out" ? i18n("used up") : i18n("left")
                opacity: 0.65
                Layout.alignment: Qt.AlignBaseline
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
        }

        // What is left, as a bar.
        Rectangle {
            Layout.fillWidth: true
            Layout.bottomMargin: 2
            implicitHeight: Math.max(4, Math.round(Kirigami.Units.smallSpacing * 1.2))
            radius: height / 2
            color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.12)
            Rectangle {
                height: parent.height
                radius: parent.radius
                width: meter.known ? parent.width * meter.info.left / 100 : 0
                color: meter.tint
                Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration } }
                Behavior on width { NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }
            }
        }
    }

    HoverHandler { id: hover }
    PlasmaComponents3.ToolTip.visible: hover.hovered && meter.known
    PlasmaComponents3.ToolTip.text: meter.known
        ? i18n("%1 limit: %2% used · resets %3", meter.info.label, 100 - meter.info.left,
            Qt.formatDateTime(new Date(meter.now + meter.info.resetsIn), meter.info.resetsIn > 86400000 ? "ddd hh:mm" : "hh:mm"))
        : ""
}
