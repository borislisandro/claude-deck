/*
 * A session in the popup: everything about it, plus quick actions.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: card

    required property var session
    required property var deckRoot

    readonly property color tint: deckRoot.statusColor(session.status, session.seen)
    readonly property string tail: deckRoot.tails[session.sid] || ""
    readonly property bool hot: hover.hovered

    implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius * 2
    color: hot
        ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.10)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.04)
    border.width: 1
    border.color: !card.session.seen
        ? Qt.rgba(card.tint.r, card.tint.g, card.tint.b, 0.7)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.08)
    Behavior on color { ColorAnimation { duration: 120 } }

    HoverHandler {
        id: hover
        onHoveredChanged: hovered ? card.deckRoot.requestPeek(card.session.sid) : card.deckRoot.requestUnpeek()
    }

    TapHandler {
        onTapped: card.deckRoot.activate(card.session.sid)
    }

    // Status stripe on the left edge.
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
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

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
                text: card.session.index
                opacity: 0.5
                font.weight: Font.DemiBold
            }
            PlasmaComponents3.Label {
                text: card.session.project
                font.bold: true
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Rectangle {
                visible: !!card.session.branch
                radius: height / 2
                color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.08)
                implicitWidth: branchLabel.implicitWidth + Kirigami.Units.largeSpacing
                implicitHeight: branchLabel.implicitHeight + 2
                PlasmaComponents3.Label {
                    id: branchLabel
                    anchors.centerIn: parent
                    text: "⎇ " + (card.session.branch || "")
                    font: Kirigami.Theme.smallFont
                    opacity: 0.8
                }
            }
            PlasmaComponents3.Label {
                text: card.deckRoot.statusLabel(card.session.status)
                color: card.tint
                font.weight: Font.DemiBold
            }
            PlasmaComponents3.Label {
                text: card.deckRoot.elapsed(card.session.since)
                opacity: 0.6
                font.features: { "tnum": 1 }
            }
        }

        PlasmaComponents3.Label {
            visible: text.length > 0
            text: card.session.error ? "⚠ " + card.session.error : (card.session.last_action || "")
            color: card.session.error ? card.tint : Kirigami.Theme.textColor
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        PlasmaComponents3.Label {
            visible: !!card.session.last_prompt
            text: "“" + (card.session.last_prompt || "") + "”"
            font.italic: true
            opacity: 0.65
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        Rectangle {
            visible: card.tail.length > 0
            Layout.fillWidth: true
            implicitHeight: tailText.implicitHeight + Kirigami.Units.smallSpacing * 2
            radius: Kirigami.Units.cornerRadius
            color: Qt.rgba(0, 0, 0, 0.32)
            clip: true
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
                maximumLineCount: 6
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 0
            opacity: card.hot ? 1 : 0.55
            Behavior on opacity { NumberAnimation { duration: 120 } }

            PlasmaComponents3.ToolButton {
                icon.name: "go-jump-symbolic"
                text: i18n("Open")
                onClicked: card.deckRoot.activate(card.session.sid)
            }
            PlasmaComponents3.ToolButton {
                visible: card.session.status === "working" || card.session.status === "needs_you"
                icon.name: "media-playback-stop-symbolic"
                text: i18n("Interrupt")
                onClicked: card.deckRoot.run("interrupt " + card.deckRoot.shellQuote(card.session.sid))
            }
            PlasmaComponents3.ToolButton {
                visible: !card.session.seen
                icon.name: "checkmark-symbolic"
                text: i18n("Mark seen")
                onClicked: card.deckRoot.run("ack " + card.deckRoot.shellQuote(card.session.sid))
            }
            Item { Layout.fillWidth: true }
            PlasmaComponents3.ToolButton {
                icon.name: "window-close-symbolic"
                display: PlasmaComponents3.AbstractButton.IconOnly
                text: i18n("Close session")
                PlasmaComponents3.ToolTip.text: text
                PlasmaComponents3.ToolTip.visible: hovered
                onClicked: {
                    card.deckRoot.run("activate " + card.deckRoot.shellQuote(card.session.sid))
                    card.deckRoot.run("close " + card.deckRoot.shellQuote(card.session.sid))
                }
            }
        }
    }
}
