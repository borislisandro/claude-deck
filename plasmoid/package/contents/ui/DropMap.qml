/*
 * Shown over a deck tab while a session is dragged onto it: the tab's panes as
 * they will be once the dragged session has left its place (the pane it shared a
 * split with takes its room, as in kitty), drawn in the tab's own proportions, and
 * under the pointer, the half of a pane the dragged session will take (left,
 * right, top or bottom of that pane, whichever edge the pointer is nearest).
 * Draws only; the popup does the hit testing.
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
    /// The tab's height over its width.
    property real aspect: 0.45
    /// The session being dragged.
    property var dragged: null
    /// The pointer, in this item's coordinates.
    property point pointer: Qt.point(-1, -1)

    readonly property real pad: Kirigami.Units.smallSpacing + 2
    /// As tall as the tab's shape asks for at this width, within reason: stacked
    /// panes stay tall enough to aim at.
    readonly property real wantedHeight: pad * 2 + headRow.implicitHeight + Kirigami.Units.smallSpacing
        + Math.max(Kirigami.Units.gridUnit * 7, Math.min(Kirigami.Units.gridUnit * 15, (width - pad * 2) * aspect))

    /// What a drop here would do: { window, side, pane } or null.
    readonly property var hit: {
        const p = area.mapFromItem(map, pointer.x, pointer.y)
        if (area.width <= 0 || area.height <= 0) return null
        return Logic.dropHit(panes, p.x / area.width, p.y / area.height, dragged ? dragged.kitty_window : -1)
    }
    readonly property bool aimed: !!(hit && hit.side)
    readonly property var hitSession: hit && hit.window ? area.sessionFor(hit.window) : null
    readonly property string sideWord: !aimed ? ""
        : hit.side === "left" ? i18n("left of") : hit.side === "right" ? i18n("right of")
        : hit.side === "top" ? i18n("above") : i18n("below")

    radius: Kirigami.Units.cornerRadius * 2
    color: Qt.rgba(Kirigami.Theme.backgroundColor.r, Kirigami.Theme.backgroundColor.g, Kirigami.Theme.backgroundColor.b, 0.97)
    border.width: 2
    border.color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, aimed ? 0.95 : 0.55)

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: map.pad
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            id: headRow
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
                text: map.aimed
                    ? i18n("Put %1 %2 %3", area.nameOf(map.dragged), map.sideWord, area.nameOf(map.hitSession))
                    : i18n("Point at a side of a pane to put %1 there", area.nameOf(map.dragged))
                font.bold: map.aimed
            }
        }

        // The tab's panes, laid out like the deck will have them.
        PaneMap {
            id: area
            Layout.fillWidth: true
            Layout.fillHeight: true
            deckRoot: map.deckRoot
            panes: map.panes
            hit: map.aimed ? map.hit : null
            dragged: map.dragged
        }
    }
}
