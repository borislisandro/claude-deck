/*
 * The card shown while hovering a session on the island. For a tab critter it
 * lists every session in the tab; the details below are its most urgent one.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Item {
    id: card

    property var session
    property var deckRoot
    property var groupSessions: []
    property int tabIndex: 0
    readonly property bool isGroup: groupSessions.length > 1

    readonly property color tint: deckRoot.statusColor(session.status, session.seen)
    readonly property string tail: deckRoot.tails[session.sid] || ""

    implicitWidth: Kirigami.Units.gridUnit * 24
    implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2

    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            spacing: Kirigami.Units.smallSpacing
            Layout.fillWidth: true
            Critter {
                px: 2
                mood: card.deckRoot.moodFor(card.session.status)
                accent: card.tint
            }
            StatusDot {
                size: 8
                status: card.session.status
                seen: card.session.seen
                color: card.tint
                waited: card.deckRoot.now - card.session.since
            }
            PlasmaComponents3.Label {
                text: card.isGroup ? i18n("Tab %1 · %2 sessions", card.tabIndex, card.groupSessions.length)
                    : card.session.index + "  " + card.session.project
                font.bold: true
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.1
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            PlasmaComponents3.Label {
                text: card.deckRoot.statusLabel(card.session.status) + " · " + card.deckRoot.elapsed(card.session.since)
                color: card.tint
                font.weight: Font.DemiBold
            }
        }

        // Every session in the tab, one line each.
        Repeater {
            model: card.isGroup ? card.groupSessions : []
            delegate: RowLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                StatusDot {
                    size: 6
                    status: modelData.status
                    seen: modelData.seen
                    color: card.deckRoot.statusColor(modelData.status, modelData.seen)
                }
                PlasmaComponents3.Label {
                    text: modelData.index + "  " + modelData.project
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                PlasmaComponents3.Label {
                    text: modelData.model || ""
                    opacity: 0.6
                    font: Kirigami.Theme.smallFont
                }
                PlasmaComponents3.Label {
                    text: card.deckRoot.statusLabel(modelData.status) + " · " + card.deckRoot.elapsed(modelData.since)
                    color: card.deckRoot.statusColor(modelData.status, modelData.seen)
                    font: Kirigami.Theme.smallFont
                }
            }
        }

        PlasmaComponents3.Label {
            visible: card.isGroup
            text: i18n("Most urgent: %1 %2", card.session.index, card.session.project)
            opacity: 0.6
            font: Kirigami.Theme.smallFont
            Layout.topMargin: Kirigami.Units.smallSpacing
        }

        PlasmaComponents3.Label {
            visible: !!card.session.branch || !!card.session.model
            text: [card.session.model || "", card.session.branch ? "⎇ " + card.session.branch : "",
                   card.session.cwd.replace(/^\/home\/[^/]+/, "~")].filter(x => x.length > 0).join("   ")
            opacity: 0.65
            elide: Text.ElideMiddle
            Layout.fillWidth: true
            font: Kirigami.Theme.smallFont
        }

        PlasmaComponents3.Label {
            visible: text.length > 0
            text: card.session.error ? "⚠ " + card.session.error : (card.session.last_action || "")
            color: card.session.error ? card.tint : Kirigami.Theme.textColor
            wrapMode: Text.Wrap
            maximumLineCount: 2
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        PlasmaComponents3.Label {
            visible: !!card.session.last_prompt
            text: "“" + (card.session.last_prompt || "") + "”"
            font.italic: true
            opacity: 0.7
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        Rectangle {
            visible: card.tail.length > 0
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.smallSpacing
            implicitHeight: tailText.implicitHeight + Kirigami.Units.smallSpacing * 2
            radius: Kirigami.Units.cornerRadius
            color: Qt.rgba(0, 0, 0, 0.35)
            border.width: 1
            border.color: Qt.rgba(card.tint.r, card.tint.g, card.tint.b, 0.35)

            PlasmaComponents3.Label {
                id: tailText
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                text: card.tail
                font.family: "monospace"
                font.pointSize: Kirigami.Theme.smallFont.pointSize
                color: "#e8dfe0"
                textFormat: Text.PlainText
                wrapMode: Text.NoWrap
                elide: Text.ElideRight
                maximumLineCount: 8
            }
        }

        PlasmaComponents3.Label {
            text: card.isGroup ? i18n("Click to open the most urgent one · click the island for all")
                : i18n("Click to open · scroll the island to switch sessions")
            opacity: 0.5
            font: Kirigami.Theme.smallFont
        }
    }
}
