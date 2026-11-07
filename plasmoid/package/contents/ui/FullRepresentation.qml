/*
 * The popup: the sessions grouped the way the deck has them, a card per tab (one
 * session: its card; several: a header and, opened up, a line per session). Each
 * session opens up onto Claude's latest message and a prompt box. Everything
 * starts folded; the popup is as tall as what it shows.
 * Drag a session onto another tab (or its own) to tile it there: a map of that
 * tab's panes shows which side of which pane it will take. Dropping it on the
 * "own tab" strip at the bottom moves it out into a tab of its own.
 */
import QtQuick
import QtQuick.Layouts
import QtQuick.Window
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.kirigami as Kirigami
import "logic.js" as Logic

PlasmaExtras.Representation {
    id: full

    required property var deckRoot

    Layout.minimumWidth: Kirigami.Units.gridUnit * 26
    Layout.preferredWidth: Kirigami.Units.gridUnit * 30
    // Plasma opens popups at their minimum size, so the minimum carries the real height.
    Layout.minimumHeight: wantedHeight
    Layout.preferredHeight: wantedHeight
    // Plasma restores the height the popup was last dragged to (popupHeight in the
    // applet's config); the maximum keeps it fitting what it shows instead.
    Layout.maximumHeight: wantedHeight
    collapseMarginsHint: true

    /// As tall as the content, within reason (the list scrolls past that). Plasma
    /// sizes the popup before its content is laid out, so until then, an estimate.
    readonly property real maxHeight: Math.min(Kirigami.Units.gridUnit * 44,
        Screen.desktopAvailableHeight > 0 ? Screen.desktopAvailableHeight * 0.85 : Infinity)
    readonly property real measuredHeight: heading.implicitHeight + body.implicitHeight + Kirigami.Units.largeSpacing
    readonly property real wantedHeight: Math.round(Math.min(maxHeight, Math.max(Kirigami.Units.gridUnit * 8,
        body.implicitHeight > 0 ? measuredHeight : Kirigami.Units.gridUnit * full.deckRoot.popupUnits)))

    // Groups as shown. Frozen while dragging: rebuilding the cards mid-drag would
    // drop the card being dragged.
    // (Assigned, never bound, so nothing updates it behind the drag's back.)
    property var shownGroups: []
    Component.onCompleted: {
        shownGroups = deckRoot.groups
        // The popup is built on its first opening, after `expanded` already flipped.
        if (deckRoot.expanded) Qt.callLater(full.blowUp)
    }
    Connections {
        target: full.deckRoot
        function onGroupsChanged() {
            if (!dragCtl.active) full.shownGroups = full.deckRoot.groups
        }
        function onExpandedChanged() {
            if (full.deckRoot.expanded) full.blowUp()
        }
    }

    // ---- Opening: the popup inflates like a balloon ------------------------
    // It swells out of the panel, stretched a little long, overshoots, wobbles
    // back to its size (squash and stretch, the cross axis lagging behind), and the
    // cards float up into place one after another.

    /// Sent at the start of the inflate: the pieces that float in start then.
    signal inflate()

    readonly property int panelEdge: Plasmoid.location
    readonly property bool sidePanel: panelEdge === PlasmaCore.Types.LeftEdge || panelEdge === PlasmaCore.Types.RightEdge
    /// Which way things float in: away from the panel (balloons rise off a bottom panel).
    readonly property real floatSign: panelEdge === PlasmaCore.Types.TopEdge ? -1 : 1

    /// Scale away from the panel, and across it.
    property real inflateAlong: 1
    property real inflateAcross: 1

    transform: Scale {
        // Grows out of the panel edge the popup hangs from.
        origin.x: full.panelEdge === PlasmaCore.Types.LeftEdge ? 0
            : full.panelEdge === PlasmaCore.Types.RightEdge ? full.width : full.width / 2
        origin.y: full.panelEdge === PlasmaCore.Types.TopEdge ? 0
            : full.sidePanel ? full.height / 2 : full.height
        xScale: full.sidePanel ? full.inflateAlong : full.inflateAcross
        yScale: full.sidePanel ? full.inflateAcross : full.inflateAlong
    }

    function blowUp() {
        if (Kirigami.Units.longDuration === 0) return   // animations turned off
        inflateAnim.restart()
        inflate()
    }

    SequentialAnimation {
        id: inflateAnim
        ScriptAction {
            script: {
                full.inflateAlong = 0.3
                full.inflateAcross = 0.55
                full.opacity = 0
            }
        }
        ParallelAnimation {
            NumberAnimation {
                target: full; property: "opacity"; to: 1
                duration: 150; easing.type: Easing.OutQuad
            }
            SequentialAnimation {
                NumberAnimation {
                    target: full; property: "inflateAlong"; to: 1.045
                    duration: 230; easing.type: Easing.OutCubic
                }
                NumberAnimation {
                    target: full; property: "inflateAlong"; to: 0.98
                    duration: 150; easing.type: Easing.InOutSine
                }
                NumberAnimation {
                    target: full; property: "inflateAlong"; to: 1.008
                    duration: 130; easing.type: Easing.InOutSine
                }
                NumberAnimation {
                    target: full; property: "inflateAlong"; to: 1
                    duration: 110; easing.type: Easing.InOutSine
                }
            }
            SequentialAnimation {
                PauseAnimation { duration: 40 }
                NumberAnimation {
                    target: full; property: "inflateAcross"; to: 1.03
                    duration: 240; easing.type: Easing.OutCubic
                }
                NumberAnimation {
                    target: full; property: "inflateAcross"; to: 0.985
                    duration: 160; easing.type: Easing.InOutSine
                }
                NumberAnimation {
                    target: full; property: "inflateAcross"; to: 1
                    duration: 150; easing.type: Easing.InOutSine
                }
            }
        }
    }

    /// Drag and drop of sessions between tabs.
    QtObject {
        id: dragCtl

        property var session: null
        readonly property bool active: session !== null
        /// The pointer, in `space` (the drag layer's) coordinates.
        property point pos: Qt.point(-1, -1)
        /// The hovered group (index into shownGroups) and its card's rect in `space`.
        property int hoverIndex: -1
        property rect cardRect: Qt.rect(0, 0, 0, 0)
        readonly property Item space: dragLayer

        readonly property var hoverGroup: hoverIndex >= 0 ? full.shownGroups[hoverIndex] || null : null
        /// Panes of the hovered tab, from kitty; until those arrive, one column per session.
        readonly property var hoverPanes: Logic.panesFor(hoverGroup, full.deckRoot.paneTabs)
        /// How many panes share the dragged session's tab (1: it's already on its own).
        readonly property int sourcePanes: {
            if (!session) return 0
            const t = session.tab !== null && session.tab !== undefined ? full.deckRoot.paneTabs[session.tab] : null
            if (t) return t.panes.length
            const g = full.shownGroups.find(g => g.sessions.some(s => s.sid === session.sid))
            return g ? g.sessions.length : 1
        }
        readonly property bool overOwnTab: ownTab.visible && within(ownTab, pos)

        function within(item, p) {
            return p.x >= item.x && p.y >= item.y && p.x <= item.x + item.width && p.y <= item.y + item.height
        }

        function begin(s) {
            full.deckRoot.requestUnpeek()
            full.deckRoot.dragging = true
            session = s
            hoverIndex = -1
            full.deckRoot.fetchPanes()
        }

        function move(p) {
            pos = p
            if (overOwnTab) {
                hoverIndex = -1
                return
            }
            // Stay on the shown map while the pointer is inside it.
            if (hoverIndex >= 0 && dropMap.visible && within(dropMap, p)) {
                refreshRect()
                return
            }
            hoverIndex = -1
            for (let i = 0; i < groupRepeater.count; i++) {
                const it = groupRepeater.itemAt(i)
                if (!it) continue
                const r = it.mapToItem(dragLayer, 0, 0)
                if (p.x >= r.x && p.y >= r.y && p.x <= r.x + it.width && p.y <= r.y + it.height) {
                    const g = full.shownGroups[i]
                    // Sessions not in a known kitty tab can't be targeted.
                    if (g && g.tab !== null && g.tab !== undefined) hoverIndex = i
                    break
                }
            }
            refreshRect()
        }

        function refreshRect() {
            const it = hoverIndex >= 0 ? groupRepeater.itemAt(hoverIndex) : null
            if (!it) return
            const r = it.mapToItem(dragLayer, 0, 0)
            cardRect = Qt.rect(r.x, r.y, it.width, it.height)
        }

        function drop() {
            const s = session
            if (s) {
                if (overOwnTab) {
                    full.deckRoot.place(s.sid, 0, "")
                } else if (dropMap.visible && dropMap.hit && dropMap.hit.window) {
                    full.deckRoot.place(s.sid, dropMap.hit.window, dropMap.hit.side)
                }
            }
            cancel()
        }

        function cancel() {
            session = null
            hoverIndex = -1
            full.deckRoot.dragging = false
            full.shownGroups = full.deckRoot.groups
        }
    }

    header: PlasmaExtras.PlasmoidHeading {
        id: heading
        RowLayout {
            anchors.fill: parent
            spacing: Kirigami.Units.smallSpacing
            Critter {
                id: headerCritter
                px: 2
                mood: (full.deckRoot.deck.counts || {}).working > 0 ? "running" : "idle"
                body: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                Layout.leftMargin: Kirigami.Units.smallSpacing
                // Hops for joy once the popup has filled out.
                transform: Translate { id: hop }
                SequentialAnimation {
                    id: hopAnim
                    PauseAnimation { duration: 300 }
                    NumberAnimation {
                        target: hop; property: "y"; to: -Kirigami.Units.smallSpacing * 2.5
                        duration: 150; easing.type: Easing.OutQuad
                    }
                    NumberAnimation {
                        target: hop; property: "y"; to: 0
                        duration: 380; easing.type: Easing.OutBounce
                    }
                }
                Connections {
                    target: full
                    function onInflate() { hopAnim.restart() }
                }
            }
            PlasmaExtras.Heading {
                level: 3
                text: "Claude Deck"
            }
            PlasmaComponents3.Label {
                text: full.deckRoot.running ? full.deckRoot.deck.summary : i18n("not running")
                opacity: 0.65
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            PlasmaComponents3.ToolButton {
                icon.name: "list-add-symbolic"
                onClicked: full.deckRoot.run(full.deckRoot.running ? "new --pick" : "toggle")
                PlasmaComponents3.ToolTip.text: i18n("New session (middle-click the island)")
                PlasmaComponents3.ToolTip.visible: hovered
            }
            PlasmaComponents3.ToolButton {
                icon.name: "view-grid-symbolic"
                enabled: full.deckRoot.sessions.length > 1
                onClicked: full.deckRoot.run("tile")
                PlasmaComponents3.ToolTip.text: i18n("Tile all sessions side by side")
                PlasmaComponents3.ToolTip.visible: hovered
            }
            PlasmaComponents3.ToolButton {
                icon.name: "tab-detach-symbolic"
                enabled: full.deckRoot.groups.length < full.deckRoot.sessions.length
                onClicked: full.deckRoot.run("untile", () => full.deckRoot.fetchPanes())
                PlasmaComponents3.ToolTip.text: i18n("Spread tiled sessions back into tabs of their own")
                PlasmaComponents3.ToolTip.visible: hovered
            }
            PlasmaComponents3.ToolButton {
                icon.name: "window-symbolic"
                onClicked: full.deckRoot.run("toggle")
                PlasmaComponents3.ToolTip.text: i18n("Show / hide the deck")
                PlasmaComponents3.ToolTip.visible: hovered
            }
        }
    }

    PlasmaComponents3.ScrollView {
        id: scroll
        anchors.fill: parent
        PlasmaComponents3.ScrollBar.horizontal.policy: PlasmaComponents3.ScrollBar.AlwaysOff

        ColumnLayout {
            id: body
            width: full.width - Kirigami.Units.largeSpacing * 2
            x: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            Item { implicitHeight: Kirigami.Units.smallSpacing }

            PlasmaExtras.PlaceholderMessage {
                visible: !full.deckRoot.running || full.deckRoot.sessions.length === 0
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.gridUnit
                Layout.bottomMargin: Kirigami.Units.gridUnit
                iconName: "utilities-terminal"
                text: full.deckRoot.running ? i18n("No sessions yet") : i18n("Claude Deck is not running")
                explanation: i18n("Meta+C opens the deck · Meta+Shift+C jumps to whatever is waiting")
            }

            // A count, not the array, so cards update in place (an armed "Close?" or
            // a hovered card survives the next state change).
            Repeater {
                id: groupRepeater
                model: full.shownGroups.length
                delegate: Loader {
                    id: groupLoader
                    required property int index
                    readonly property var group: full.shownGroups[index] || null
                    Layout.fillWidth: true
                    active: group !== null
                    sourceComponent: group && group.sessions.length === 1 ? singleCard : groupCard
                    // Floats up after the one above it.
                    transform: Rise {
                        id: cardRise
                        target: groupLoader
                        delay: 110 + Math.min(groupLoader.index, 8) * 50
                        distance: Kirigami.Units.gridUnit * 1.3 * full.floatSign
                    }
                    Connections {
                        target: full
                        function onInflate() { cardRise.play() }
                    }
                    Component {
                        id: singleCard
                        SessionCard {
                            session: (groupLoader.group && groupLoader.group.sessions[0]) || full.deckRoot.noSession
                            deckRoot: full.deckRoot
                            dnd: dragCtl
                        }
                    }
                    Component {
                        id: groupCard
                        GroupCard {
                            group: groupLoader.group || full.deckRoot.noGroup
                            deckRoot: full.deckRoot
                            dnd: dragCtl
                        }
                    }
                }
            }

            PlasmaComponents3.Label {
                visible: full.deckRoot.sessions.length > 0
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.smallSpacing
                Layout.bottomMargin: Kirigami.Units.smallSpacing
                horizontalAlignment: Text.AlignHCenter
                text: i18n("Click a session to open it · drag it onto another tab to tile it there")
                opacity: 0.5
                font: Kirigami.Theme.smallFont
                wrapMode: Text.Wrap
            }
        }
    }

    // Above the list while dragging: the drop map, the "own tab" strip and the ghost.
    Item {
        id: dragLayer
        anchors.fill: parent
        z: 10
        visible: dragCtl.active

        DropMap {
            id: dropMap
            deckRoot: full.deckRoot
            visible: dragCtl.active && dragCtl.hoverGroup !== null
            group: dragCtl.hoverGroup
            panes: dragCtl.hoverPanes
            dragged: dragCtl.session
            pointer: Qt.point(dragCtl.pos.x - x, dragCtl.pos.y - y)

            readonly property real bottomLimit: ownTab.visible ? ownTab.y - Kirigami.Units.smallSpacing : dragLayer.height
            x: dragCtl.cardRect.x
            width: dragCtl.cardRect.width
            height: Math.min(bottomLimit, Math.max(dragCtl.cardRect.height, Kirigami.Units.gridUnit * 8))
            // Centered on the card, kept inside the popup.
            y: Math.max(0, Math.min(bottomLimit - height, dragCtl.cardRect.y + dragCtl.cardRect.height / 2 - height / 2))
        }

        // Drop here: the session moves out into a tab of its own.
        Rectangle {
            id: ownTab
            visible: dragCtl.active && dragCtl.sourcePanes > 1
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.margins: Kirigami.Units.largeSpacing
            height: Kirigami.Units.gridUnit * 2.6
            radius: Kirigami.Units.cornerRadius * 2
            readonly property color tint: Kirigami.Theme.highlightColor
            color: Qt.rgba(Kirigami.Theme.backgroundColor.r, Kirigami.Theme.backgroundColor.g, Kirigami.Theme.backgroundColor.b, 0.97)
            border.width: 2
            border.color: Qt.rgba(tint.r, tint.g, tint.b, dragCtl.overOwnTab ? 1 : 0.45)

            Rectangle {
                anchors.fill: parent
                anchors.margins: 2
                radius: parent.radius
                color: ownTab.tint
                opacity: dragCtl.overOwnTab ? 0.35 : 0.06
                Behavior on opacity { NumberAnimation { duration: 100 } }
            }
            RowLayout {
                anchors.centerIn: parent
                spacing: Kirigami.Units.smallSpacing
                Kirigami.Icon {
                    source: "tab-detach-symbolic"
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: Kirigami.Units.iconSizes.small
                }
                PlasmaComponents3.Label {
                    text: dragCtl.session ? i18n("Drop here to give #%1 %2 a tab of its own", dragCtl.session.index, dragCtl.session.project) : ""
                    font.bold: dragCtl.overOwnTab
                }
            }
        }

        // What's being dragged, under the pointer.
        Rectangle {
            id: ghost
            x: dragCtl.pos.x + 14
            y: dragCtl.pos.y + 10
            width: ghostRow.implicitWidth + Kirigami.Units.largeSpacing * 2
            height: ghostRow.implicitHeight + Kirigami.Units.smallSpacing * 2
            radius: height / 2
            readonly property color tint: dragCtl.session ? full.deckRoot.statusColor(dragCtl.session.status, dragCtl.session.seen) : Kirigami.Theme.textColor
            color: Qt.rgba(Kirigami.Theme.backgroundColor.r, Kirigami.Theme.backgroundColor.g, Kirigami.Theme.backgroundColor.b, 0.95)
            border.width: 1
            border.color: tint
            RowLayout {
                id: ghostRow
                anchors.centerIn: parent
                spacing: Kirigami.Units.smallSpacing
                Critter {
                    px: 2
                    mood: dragCtl.session ? full.deckRoot.moodFor(dragCtl.session.status) : "idle"
                    accent: ghost.tint
                    body: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                }
                PlasmaComponents3.Label {
                    text: dragCtl.session ? "#" + dragCtl.session.index + " " + dragCtl.session.project : ""
                    font.bold: true
                }
            }
        }
    }

    // Scroll the list while dragging near its top or bottom edge.
    Timer {
        interval: 30
        repeat: true
        running: dragCtl.active
        onTriggered: {
            const f = scroll.contentItem
            if (!f || f.contentY === undefined || (dropMap.visible && dragCtl.within(dropMap, dragCtl.pos))) return
            const edge = Kirigami.Units.gridUnit * 1.5
            const bottom = ownTab.visible ? ownTab.y : dragLayer.height
            const maxY = Math.max(0, f.contentHeight - f.height)
            let dy = 0
            if (dragCtl.pos.y < edge) dy = -10
            else if (dragCtl.pos.y > bottom - edge && dragCtl.pos.y < bottom) dy = 10
            if (dy !== 0) {
                f.contentY = Math.max(0, Math.min(maxY, f.contentY + dy))
                dragCtl.move(dragCtl.pos)
            }
        }
    }
}
