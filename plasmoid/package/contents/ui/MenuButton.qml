/*
 * The island's popup button: a small pixel chevron at one end of the pill, set
 * apart from the critters by a thin divider. Its click area runs out to the end
 * of the pill, so it's easy to hit without aiming.
 */
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami

PlasmaCore.ToolTipArea {
    id: button

    required property var deckRoot
    property int px: 2
    /// Extra click room toward the pill's end, and toward the critters.
    property real outerPad: 0
    property real innerPad: 0
    /// Which end of the pill this button sits at (the divider faces the critters).
    property bool onLeft: false

    readonly property bool hot: mouse.containsMouse
    readonly property bool open: deckRoot.expanded
    /// The popup opens away from the panel edge; the chevron points that way.
    readonly property bool pointsUp: Plasmoid.location === PlasmaCore.Types.BottomEdge
    readonly property color ink: deckRoot.unseen > 0
        ? (deckRoot.deck.counts && deckRoot.deck.counts.needs_you > 0 ? Plasmoid.configuration.colorNeedsYou : Plasmoid.configuration.colorYourTurn)
        : Kirigami.Theme.textColor

    readonly property real gap: px * 2
    implicitWidth: chevron.width + gap + divider.width
    implicitHeight: px * 8

    mainText: button.open ? i18n("Close the session list") : i18n("Show sessions")
    subText: deckRoot.running ? deckRoot.deck.summary : ""
    location: Plasmoid.location

    // Thin divider between the critters and the button.
    Rectangle {
        id: divider
        width: 1
        height: button.px * 6
        anchors.verticalCenter: parent.verticalCenter
        x: button.onLeft ? parent.width - width : 0
        color: Kirigami.Theme.textColor
        opacity: 0.18
    }

    // Hover halo.
    Rectangle {
        anchors.centerIn: chevron
        width: chevron.width + button.px * 4
        height: width
        radius: width / 2
        color: Kirigami.Theme.textColor
        opacity: button.hot ? 0.14 : (button.open ? 0.08 : 0)
        Behavior on opacity { NumberAnimation { duration: Kirigami.Units.shortDuration } }
    }

    // A 5x3 pixel chevron.
    Item {
        id: chevron
        width: button.px * 5
        height: button.px * 3
        anchors.verticalCenter: parent.verticalCenter
        x: button.onLeft ? 0 : parent.width - width
        // Down by default; flip for a bottom panel, and again while the popup is open.
        rotation: (button.pointsUp ? 180 : 0) + (button.open ? 180 : 0)
        Behavior on rotation { NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }
        opacity: button.hot || button.open ? 1 : 0.7
        Behavior on opacity { NumberAnimation { duration: Kirigami.Units.shortDuration } }

        Repeater {
            model: [[0, 0], [4, 0], [1, 1], [3, 1], [2, 2]]
            delegate: Rectangle {
                required property var modelData
                x: modelData[0] * button.px
                y: modelData[1] * button.px
                width: button.px
                height: button.px
                color: button.ink
                Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }
            }
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        anchors.leftMargin: -(button.onLeft ? button.outerPad : button.innerPad)
        anchors.rightMargin: -(button.onLeft ? button.innerPad : button.outerPad)
        anchors.topMargin: -button.px * 10
        anchors.bottomMargin: -button.px * 10
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        // The popup closes itself when the press takes focus away, so decide on press.
        property bool wasExpanded: false
        onPressed: wasExpanded = button.deckRoot.expanded
        onClicked: button.deckRoot.expanded = !wasExpanded
    }
}
