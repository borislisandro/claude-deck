/*
 * The card shown while hovering a session on the island.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Item {
    id: card

    property var session
    property var deckRoot

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
                text: card.session.index + "  " + card.session.project
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

        PlasmaComponents3.Label {
            visible: !!card.session.branch
            text: "⎇ " + (card.session.branch || "") + "   " + card.session.cwd.replace(/^\/home\/[^/]+/, "~")
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
            text: i18n("Click to open · scroll the island to switch sessions")
            opacity: 0.5
            font: Kirigami.Theme.smallFont
        }
    }
}
