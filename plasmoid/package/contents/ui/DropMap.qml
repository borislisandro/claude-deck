/*
 * Shown over a deck tab while a session is dragged onto it: the tab's panes as
 * they sit in the deck, and under the pointer, the half of a pane the dragged
 * session will take (left, right, top or bottom of that pane, whichever edge
 * the pointer is nearest). Draws only; the popup does the hit testing.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami
import "logic.js" as Logic

Rectangle {
    id: map

    required property var deckRoot
    /// The tab being hovered (a popup group) and its panes ({ window, x, y, w, h }, 0 to 1).
    property var group: null
    property var panes: []
    /// The session being dragged.
    property var dragged: null
    /// The pointer, in this item's coordinates.
    property point pointer: Qt.point(-1, -1)

    readonly property real gap: 3
    readonly property int draggedWindow: dragged ? dragged.kitty_window : -1

    /// What a drop here would do: { window, side, pane } or { self: true } or null.
    readonly property var hit: {
        const p = area.mapFromItem(map, pointer.x, pointer.y)
        if (area.width <= 0 || area.height <= 0) return null
        return Logic.dropHit(panes, p.x / area.width, p.y / area.height, draggedWindow)
    }

    function sessionFor(window) {
        return deckRoot.sessions.find(s => s.kitty_window === window) || null
    }
    function nameOf(s) {
        return s ? "#" + s.index + " " + s.project : i18n("pane")
    }
    readonly property var hitSession: hit && hit.window ? sessionFor(hit.window) : null
    readonly property string sideWord: !hit || !hit.side ? ""
        : hit.side === "left" ? i18n("left of") : hit.side === "right" ? i18n("right of")
        : hit.side === "top" ? i18n("above") : i18n("below")

    radius: Kirigami.Units.cornerRadius * 2
    color: Qt.rgba(Kirigami.Theme.backgroundColor.r, Kirigami.Theme.backgroundColor.g, Kirigami.Theme.backgroundColor.b, 0.97)
    border.width: 2
    border.color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, map.hit && map.hit.side ? 0.95 : 0.55)

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing + 2
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Icon {
                source: "view-split-left-right-symbolic"
                implicitWidth: Kirigami.Units.iconSizes.small
                implicitHeight: Kirigami.Units.iconSizes.small
                opacity: 0.7
            }
            PlasmaComponents3.Label {
                text: map.group ? i18n("Tab %1", map.group.tabIndex) : ""
                font.weight: Font.DemiBold
                opacity: 0.6
            }
            PlasmaComponents3.Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: !map.hit ? i18n("Point at the edge of a pane")
                    : map.hit.self ? i18n("This is where it is now")
                    : i18n("Put %1 %2 %3", map.nameOf(map.dragged), map.sideWord, map.nameOf(map.hitSession))
                font.bold: !!(map.hit && map.hit.side)
            }
        }

        // The tab's panes, laid out like the deck has them.
        Item {
            id: area
            Layout.fillWidth: true
            Layout.fillHeight: true

            Repeater {
                model: map.panes
                delegate: Rectangle {
                    id: paneRect
                    required property var modelData
                    readonly property var session: map.sessionFor(modelData.window)
                    readonly property bool isSelf: modelData.window === map.draggedWindow
                    readonly property bool isHit: !!(map.hit && map.hit.window === modelData.window)
                    readonly property color tint: session ? map.deckRoot.statusColor(session.status, session.seen) : Kirigami.Theme.disabledTextColor

                    x: modelData.x * area.width + map.gap / 2
                    y: modelData.y * area.height + map.gap / 2
                    width: Math.max(4, modelData.w * area.width - map.gap)
                    height: Math.max(4, modelData.h * area.height - map.gap)
                    radius: Kirigami.Units.cornerRadius
                    color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, isSelf ? 0.03 : 0.08)
                    border.width: 1
                    border.color: Qt.rgba(tint.r, tint.g, tint.b, isSelf ? 0.35 : 0.7)
                    opacity: isSelf ? 0.55 : 1
                    clip: true

                    // Status stripe, like the cards.
                    Rectangle {
                        width: 3
                        radius: 1
                        anchors.left: parent.left
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        anchors.margins: 3
                        color: paneRect.tint
                    }

                    // Where the dragged session will go: half of this pane.
                    Rectangle {
                        visible: paneRect.isHit
                        readonly property string side: paneRect.isHit ? map.hit.side : ""
                        x: side === "right" ? parent.width / 2 : 0
                        y: side === "bottom" ? parent.height / 2 : 0
                        width: side === "left" || side === "right" ? parent.width / 2 : parent.width
                        height: side === "top" || side === "bottom" ? parent.height / 2 : parent.height
                        radius: Kirigami.Units.cornerRadius
                        color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.45)
                        border.width: 2
                        border.color: Kirigami.Theme.highlightColor
                        Behavior on x { NumberAnimation { duration: 90 } }
                        Behavior on y { NumberAnimation { duration: 90 } }
                        Behavior on width { NumberAnimation { duration: 90 } }
                        Behavior on height { NumberAnimation { duration: 90 } }

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

                    // The pane's own label, in the half it keeps.
                    PlasmaComponents3.Label {
                        readonly property string side: paneRect.isHit ? map.hit.side : ""
                        x: (side === "left" ? parent.width / 2 : 0) + 8
                        y: side === "top" ? parent.height / 2 + 2 : 4
                        width: (side === "left" || side === "right" ? parent.width / 2 : parent.width) - 12
                        elide: Text.ElideRight
                        text: paneRect.isSelf ? i18n("%1 (moving)", map.nameOf(paneRect.session)) : map.nameOf(paneRect.session)
                        font: Kirigami.Theme.smallFont
                        opacity: 0.85
                    }
                }
            }
        }
    }
}
