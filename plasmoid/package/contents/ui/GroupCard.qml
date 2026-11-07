/*
 * A deck tab holding several sessions: one header for the tab, one row per session.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: card

    required property var group
    required property var deckRoot

    readonly property var lead: group.top
    readonly property color tint: deckRoot.statusColor(lead.status, lead.seen)
    readonly property bool folded: deckRoot.foldedGroups[group.key] === true
    readonly property bool anyBusy: group.counts.working + group.counts.needs_you > 0
    readonly property bool mixedProjects: group.projects.length > 1

    implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius * 2
    color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.04)
    border.width: 1
    border.color: group.unseen > 0
        ? Qt.rgba(card.tint.r, card.tint.g, card.tint.b, 0.7)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.08)

    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: Kirigami.Units.smallSpacing
        width: 3
        radius: 2
        color: card.tint
    }

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        anchors.leftMargin: Kirigami.Units.largeSpacing + 4
        spacing: 2

        RowLayout {
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
                icon.name: "go-jump-symbolic"
                display: PlasmaComponents3.AbstractButton.IconOnly
                text: i18n("Open the most urgent session in this tab")
                onClicked: card.deckRoot.activate(card.lead.sid)
                PlasmaComponents3.ToolTip.text: text
                PlasmaComponents3.ToolTip.visible: hovered
            }
            ConfirmButton {
                tip: i18n("Close this tab and its %1 sessions", card.group.sessions.length)
                needsConfirm: true
                confirmText: i18n("Close %1?", card.group.sessions.length)
                onConfirmed: card.deckRoot.closeGroup(card.group)
            }
            PlasmaComponents3.ToolButton {
                icon.name: card.folded ? "arrow-down-symbolic" : "arrow-up-symbolic"
                display: PlasmaComponents3.AbstractButton.IconOnly
                text: card.folded ? i18n("Show sessions") : i18n("Hide sessions")
                onClicked: card.deckRoot.toggleFold(card.group.key)
                PlasmaComponents3.ToolTip.text: text
                PlasmaComponents3.ToolTip.visible: hovered
            }
        }

        Repeater {
            model: card.folded ? [] : card.group.sessions
            delegate: SessionRow {
                required property var modelData
                session: modelData
                deckRoot: card.deckRoot
                showProject: card.mixedProjects
                Layout.fillWidth: true
            }
        }
    }
}
