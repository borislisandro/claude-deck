/*
 * The island: a critter + light per session. With more sessions than
 * "collapseAfter", one critter per deck tab (its most urgent session, with a
 * count); with more tabs than that too, a count per state.
 * Zero width while the deck isn't running.
 * The popup opens from its own small button at one end of the pill, so a click
 * that just misses a critter never opens it by surprise.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami
import "logic.js" as Logic

Item {
    id: compact

    required property var deckRoot

    readonly property bool running: deckRoot.running
    readonly property var sessions: deckRoot.sessions
    readonly property var groups: deckRoot.groups
    /// "sessions" | "tabs" | "counts"
    readonly property string mode: Logic.islandMode(sessions.length, groups.length, Plasmoid.configuration.collapseAfter)
    readonly property bool collapsed: mode === "counts"
    /// Critter pixel size: whole pixels, as large as the pill allows.
    readonly property int px: Math.max(2, Math.floor(height * 0.84 / 11))
    readonly property real glyph: px * 8
    readonly property real pad: Math.round(px * 4)
    readonly property bool menuOnLeft: Plasmoid.configuration.menuButtonSide === "left"
    readonly property color attention: deckRoot.unseen > 0
        ? (deckRoot.deck.counts && deckRoot.deck.counts.needs_you > 0 ? Plasmoid.configuration.colorNeedsYou : Plasmoid.configuration.colorYourTurn)
        : "transparent"

    // Not `visible: running` here: Plasma's CompactApplet sets this item's visible to
    // true when it adopts it, which breaks that binding for good, and the pill (centered,
    // unclipped) then spills out of the zero width as a lone idle critter. The children
    // carry it instead.
    // Follows the pill, which animates its own width: animating this too would chase
    // a moving target and lag behind it.
    readonly property real wantedWidth: running ? pill.width + Kirigami.Units.smallSpacing : 0
    Layout.minimumWidth: wantedWidth
    Layout.preferredWidth: wantedWidth
    Layout.maximumWidth: wantedWidth
    Layout.fillHeight: true

    // Background clicks: middle starts a session, wheel switches. A left click here
    // does nothing: the popup has its own button, and critters own the space around them.
    MouseArea {
        anchors.fill: parent
        visible: compact.running
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        onClicked: mouse => {
            if (mouse.button === Qt.MiddleButton) {
                compact.deckRoot.run("new --pick")
            }
        }
        property real wheelAcc: 0
        onWheel: wheel => {
            wheelAcc += wheel.angleDelta.y
            if (Math.abs(wheelAcc) >= 120) {
                compact.deckRoot.cycle(wheelAcc > 0 ? -1 : 1)
                wheelAcc = 0
            }
        }
    }

    Rectangle {
        id: pill
        visible: compact.running
        anchors.centerIn: parent
        height: Math.min(compact.height, Math.max(Math.round(compact.height * 0.74), compact.px * 11 + 4))
        width: content.implicitWidth + compact.pad * 2
        radius: height / 2
        // The critters re-lay out at once while the pill widens to fit them: kept
        // inside it, they never spill over the panel's other widgets meanwhile.
        clip: true
        color: Plasmoid.configuration.ownBackground
            ? Qt.rgba(Kirigami.Theme.backgroundColor.r, Kirigami.Theme.backgroundColor.g, Kirigami.Theme.backgroundColor.b, 0.35)
            : "transparent"
        border.width: compact.deckRoot.unseen > 0 ? 1 : 0
        border.color: compact.deckRoot.unseen > 0
            ? Qt.rgba(compact.attention.r, compact.attention.g, compact.attention.b, 0.75)
            : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.10)
        Behavior on border.color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }
        Behavior on width { NumberAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }

        // A slow glow across the pill while anything waits unseen, on the lights' beat
        // (StatusDot): one swell every two. Off with Plasma's animations; the border
        // still says it.
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: compact.attention
            opacity: 0
            visible: glow.running
            SequentialAnimation on opacity {
                id: glow
                running: compact.deckRoot.unseen > 0 && Kirigami.Units.longDuration > 1
                loops: Animation.Infinite
                NumberAnimation { from: 0; to: 0.16; duration: 1200; easing.type: Easing.InOutSine }
                NumberAnimation { from: 0.16; to: 0; duration: 1200; easing.type: Easing.InOutSine }
            }
        }

        RowLayout {
            id: content
            anchors.centerIn: parent
            spacing: compact.px * 3

            MenuButton {
                visible: compact.menuOnLeft
                deckRoot: compact.deckRoot
                px: compact.px
                outerPad: compact.pad + Kirigami.Units.smallSpacing
                innerPad: content.spacing / 2
                onLeft: true
                Layout.alignment: Qt.AlignVCenter
            }

            // Deck running but no sessions yet (picker open), or collapsed: one critter.
            Critter {
                visible: compact.sessions.length === 0 || compact.collapsed
                px: compact.px
                mood: compact.sessions.length === 0 ? "idle"
                    : ((compact.deckRoot.deck.counts || {}).working > 0 ? "running" : "resting")
                body: Plasmoid.configuration.claudeColoredGlyph ? "#d97757" : Kirigami.Theme.textColor
                animated: Plasmoid.configuration.spinWhileWorking
                Layout.alignment: Qt.AlignVCenter
            }

            // Driven by a count, not the array: a new state then updates each critter
            // in place instead of recreating it (which restarted its animation and
            // closed a hover card under the pointer on every hook event).
            Repeater {
                id: sessionIcons
                model: compact.mode === "sessions" ? compact.sessions.length : 0
                /// How many it showed before the count last changed: icons past that are new.
                property int settled: 0
                onCountChanged: Qt.callLater(() => settled = count)
                delegate: SessionIcon {
                    required property int index
                    fadeIn: index >= sessionIcons.settled
                    session: compact.sessions[index] || compact.deckRoot.noSession
                    deckRoot: compact.deckRoot
                    px: compact.px
                    hitPadX: content.spacing / 2
                    hitPadY: compact.height
                    Layout.alignment: Qt.AlignVCenter
                }
            }

            // One critter per tab: the tab's most urgent session, and how many it holds.
            Repeater {
                id: tabIcons
                model: compact.mode === "tabs" ? compact.groups.length : 0
                property int settled: 0
                onCountChanged: Qt.callLater(() => settled = count)
                delegate: SessionIcon {
                    required property int index
                    fadeIn: index >= tabIcons.settled
                    readonly property var group: compact.groups[index] || null
                    session: group ? group.top : compact.deckRoot.noSession
                    groupSessions: group ? group.sessions : []
                    tabIndex: group ? group.tabIndex : 0
                    deckRoot: compact.deckRoot
                    px: compact.px
                    hitPadX: content.spacing / 2
                    hitPadY: compact.height
                    Layout.alignment: Qt.AlignVCenter
                }
            }

            // Collapsed: one count per state, most urgent first.
            Repeater {
                model: compact.collapsed ? [
                    { key: "needs_you", seen: false },
                    { key: "your_turn", seen: false },
                    { key: "working", seen: true },
                    { key: "background", seen: true },
                    { key: "exited", seen: true }
                ] : []
                delegate: RowLayout {
                    required property var modelData
                    readonly property int n: (compact.deckRoot.deck.counts || {})[modelData.key] || 0
                    visible: n > 0
                    spacing: 2
                    StatusDot {
                        size: Math.round(compact.glyph * 0.5)
                        status: modelData.key
                        seen: modelData.key !== "your_turn" || compact.deckRoot.unseen === 0
                        color: compact.deckRoot.statusColor(modelData.key, seen)
                    }
                    PlasmaComponents3.Label {
                        text: n
                        font.weight: Font.DemiBold
                        font.pixelSize: Math.round(compact.glyph * 0.75)
                    }
                }
            }

            MenuButton {
                visible: !compact.menuOnLeft
                deckRoot: compact.deckRoot
                px: compact.px
                outerPad: compact.pad + Kirigami.Units.smallSpacing
                innerPad: content.spacing / 2
                onLeft: false
                Layout.alignment: Qt.AlignVCenter
            }
        }
    }
}
