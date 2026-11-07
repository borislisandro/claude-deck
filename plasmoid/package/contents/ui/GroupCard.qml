/*
 * A deck tab holding several sessions: one header for the tab and, once opened
 * (folded is the default), one row per session. A click on the header opens or
 * folds it; each row opens the session it shows.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: card

    required property var group
    required property var deckRoot
    /// The popup's drag controller, handed to each row.
    property var dnd: null

    readonly property var lead: group.top
    readonly property color tint: deckRoot.statusColor(lead.status, lead.seen)
    readonly property bool open: deckRoot.isOpen(group.key)
    readonly property bool anyBusy: group.counts.working + group.counts.needs_you > 0
    readonly property bool mixedProjects: group.projects.length > 1

    implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius * 2
    // Lights up under the pointer like a session card while its header is hovered
    // (the rows, once open, light up on their own).
    color: headerHover.hovered
        ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.10)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.04)
    border.width: 1
    border.color: group.unseen > 0
        ? Qt.rgba(card.tint.r, card.tint.g, card.tint.b, 0.7)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.08)
    Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
    Behavior on border.color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }

    // Its sessions drop in under the header when it opens.
    onOpenChanged: if (open) rowsRise.play()

    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: Kirigami.Units.smallSpacing
        width: 3
        radius: 2
        color: card.tint
        Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }
    }

    // A click on the header (not its buttons, which sit above) opens or folds the tab.
    MouseArea {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: col.y + headerRow.y + headerRow.height + Kirigami.Units.smallSpacing
        cursorShape: Qt.PointingHandCursor
        onClicked: card.deckRoot.toggleOpen(card.group.key)
        // Passive, so it stays hovered over the header's buttons too.
        HoverHandler { id: headerHover }
    }

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        anchors.leftMargin: Kirigami.Units.largeSpacing + 4
        spacing: 2

        RowLayout {
            id: headerRow
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Critter {
                px: 2
                mood: card.deckRoot.moodFor(card.lead.status)
                accent: card.tint
            }
            PlasmaComponents3.Label {
                text: i18n("Tab %1", card.group.tabIndex)
                opacity: 0.55
                font.weight: Font.DemiBold
            }
            PlasmaComponents3.Label {
                text: card.group.projects.join(", ")
                font.bold: true
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            // One chip per state present, most urgent first.
            Repeater {
                model: ["needs_you", "your_turn", "working", "exited"]
                delegate: Chip {
                    required property string modelData
                    readonly property int n: card.group.counts[modelData] || 0
                    visible: n > 0
                    text: "● " + n
                    tint: card.deckRoot.statusColor(modelData, modelData !== "your_turn" || card.group.unseen === 0)
                    strength: 0.18
                }
            }
            Chip {
                text: card.group.model
                tint: "#d97757"
                strength: 0.16
            }
            PlasmaComponents3.ToolButton {
                icon.name: "tab-detach-symbolic"
                display: PlasmaComponents3.AbstractButton.IconOnly
                text: i18n("Spread these sessions into tabs of their own")
                onClicked: card.deckRoot.untileGroup(card.group)
                PlasmaComponents3.ToolTip.text: text
                PlasmaComponents3.ToolTip.visible: hovered
            }
            ConfirmButton {
                tip: i18n("Close this tab and its %1 sessions", card.group.sessions.length)
                needsConfirm: true
                confirmText: i18n("Close %1?", card.group.sessions.length)
                onConfirmed: card.deckRoot.closeGroup(card.group)
            }
            FoldButton {
                open: card.open
                openText: i18n("Hide sessions")
                closedText: i18n("Show the %1 sessions", card.group.sessions.length)
                onClicked: card.deckRoot.toggleOpen(card.group.key)
            }
        }

        ColumnLayout {
            id: rows
            Layout.fillWidth: true
            visible: card.open
            spacing: col.spacing
            transform: Rise {
                id: rowsRise
                target: rows
                distance: -Kirigami.Units.gridUnit * 0.4
            }

            Repeater {
                model: card.open ? card.group.sessions.length : 0
                delegate: SessionRow {
                    required property int index
                    session: card.group.sessions[index] || card.deckRoot.noSession
                    deckRoot: card.deckRoot
                    dnd: card.dnd
                    showProject: card.mixedProjects
                    Layout.fillWidth: true
                }
            }
        }
    }
}
