/*
 * A deck tab's panes, drawn where they sit in it. Three uses: the small picture
 * of the split in a tab's header (no labels), the map an opened tab shows above
 * its sessions (labelled; a click opens a pane, a drag picks it up to move it),
 * and the drop map while a session is dragged (one half of a pane lit: where the
 * dragged session will go).
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Item {
    id: map

    required property var deckRoot
    /// [{ window, x, y, w, h }], each from 0 to 1 inside the tab.
    property var panes: []
    property real gap: labels ? 3 : 1.5
    /// Each pane's session written on it; off: just the shape of the split.
    property bool labels: true
    /// The popup's drag controller: panes then open on a click and move on a drag.
    property var dnd: null
    /// A drop being aimed ({ window, side }): that half of that pane lights up.
    property var hit: null
    /// The session being dragged (named in the lit half).
    property var dragged: null
    /// A pane to light up (a window id), e.g. while its line in the list is hovered.
    property int litWindow: -1
    /// The pane under the pointer (a window id), or -1.
    property int hoveredWindow: -1

    function sessionFor(window) {
        return deckRoot.sessions.find(s => s.kitty_window === window) || null
    }
    function nameOf(s) {
        return s ? "#" + s.index + " " + s.project : i18n("pane")
    }

    Repeater {
        model: map.panes
        delegate: Rectangle {
            id: tile
            required property var modelData
            readonly property var session: map.sessionFor(modelData.window)
            readonly property bool isHit: !!(map.hit && map.hit.window === modelData.window)
            readonly property bool lit: map.litWindow === modelData.window || tileHover.hovered
            readonly property color tint: session ? map.deckRoot.statusColor(session.status, session.seen) : Kirigami.Theme.disabledTextColor

            x: modelData.x * map.width + map.gap / 2
            y: modelData.y * map.height + map.gap / 2
            width: Math.max(2, modelData.w * map.width - map.gap)
            height: Math.max(2, modelData.h * map.height - map.gap)
            radius: map.labels ? Kirigami.Units.cornerRadius : 1
            color: !map.labels ? Qt.rgba(tint.r, tint.g, tint.b, 0.45)
                : lit ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.16)
                : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.07)
            border.width: map.labels ? 1 : 0
            border.color: lit ? Kirigami.Theme.highlightColor : Qt.rgba(tint.r, tint.g, tint.b, 0.7)
            clip: map.labels
            Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

            HoverHandler {
                id: tileHover
                enabled: map.dnd !== null
                onHoveredChanged: {
                    if (hovered) map.hoveredWindow = tile.modelData.window
                    else if (map.hoveredWindow === tile.modelData.window) map.hoveredWindow = -1
                }
            }
            DragGrip {
                anchors.fill: parent
                enabled: map.dnd !== null && tile.session !== null
                visible: enabled
                dnd: map.dnd
                session: tile.session
                onTapped: map.deckRoot.activate(tile.session.sid)
                PlasmaComponents3.ToolTip.text: i18n("Click to open · drag beside another pane to move it")
                PlasmaComponents3.ToolTip.visible: tileHover.hovered && !pressed
                PlasmaComponents3.ToolTip.delay: Kirigami.Units.toolTipDelay
            }

            // Status stripe, like the cards.
            Rectangle {
                visible: map.labels
                width: 3
                radius: 1
                anchors.left: parent.left
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                anchors.margins: 3
                color: tile.tint
            }

            // Who is in it, in the half it keeps when a drop is aimed at it.
            ColumnLayout {
                visible: map.labels
                readonly property string side: tile.isHit ? map.hit.side : ""
                x: (side === "left" ? tile.width / 2 : 0) + 9
                y: (side === "top" ? tile.height / 2 : 0) + 3
                width: (side === "left" || side === "right" ? tile.width / 2 : tile.width) - 14
                height: (side === "top" || side === "bottom" ? tile.height / 2 : tile.height) - 6
                spacing: 0
                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: map.nameOf(tile.session)
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
                PlasmaComponents3.Label {
                    id: activity
                    // Only while there is room for it.
                    visible: !!tile.session && parent.height >= implicitHeight * 2.6
                    Layout.fillWidth: true
                    text: !tile.session ? ""
                        : tile.session.error ? "⚠ " + tile.session.error
                        : (tile.session.last_action || tile.session.last_prompt || map.deckRoot.statusLabel(tile.session.status))
                    font: Kirigami.Theme.smallFont
                    opacity: 0.65
                    elide: Text.ElideRight
                }
                PlasmaComponents3.Label {
                    visible: !!tile.session && parent.height >= activity.implicitHeight * 3.8
                    text: tile.session ? map.deckRoot.statusLabel(tile.session.status) : ""
                    color: tile.tint
                    font.pointSize: Kirigami.Theme.smallFont.pointSize
                    font.weight: Font.DemiBold
                }
                Item { Layout.fillHeight: true }
            }

            // Where the dragged session will go: half of this pane.
            Rectangle {
                visible: tile.isHit
                readonly property string side: tile.isHit ? map.hit.side : ""
                x: side === "right" ? parent.width / 2 : 0
                y: side === "bottom" ? parent.height / 2 : 0
                width: side === "left" || side === "right" ? parent.width / 2 : parent.width
                height: side === "top" || side === "bottom" ? parent.height / 2 : parent.height
                radius: Kirigami.Units.cornerRadius
                color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.45)
                border.width: 2
                border.color: Kirigami.Theme.highlightColor
                // Slides to the half under the pointer: quick, so it keeps up with it.
                Behavior on x { NumberAnimation { duration: Kirigami.Units.shortDuration; easing.type: Easing.OutCubic } }
                Behavior on y { NumberAnimation { duration: Kirigami.Units.shortDuration; easing.type: Easing.OutCubic } }
                Behavior on width { NumberAnimation { duration: Kirigami.Units.shortDuration; easing.type: Easing.OutCubic } }
                Behavior on height { NumberAnimation { duration: Kirigami.Units.shortDuration; easing.type: Easing.OutCubic } }

                PlasmaComponents3.Label {
                    anchors.centerIn: parent
                    width: parent.width - 6
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: map.nameOf(map.dragged)
                    font.bold: true
                    color: Kirigami.Theme.highlightedTextColor
                }
            }
        }
    }
}
