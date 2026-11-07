/*
 * A deck tab holding one session. Folded (the default): its state in two lines.
 * Open (the arrow): Claude's latest message, a prompt box and the actions.
 * A click anywhere else on the card opens the session; a drag moves it.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: card

    required property var session
    required property var deckRoot
    /// The popup's drag controller.
    property var dnd: null

    readonly property color tint: deckRoot.statusColor(session.status, session.seen)
    readonly property bool hot: hover.hovered
    readonly property bool open: deckRoot.isOpen(session.sid)
    readonly property bool busy: session.status === "working" || session.status === "needs_you"
    readonly property string activity: session.error ? "⚠ " + session.error
        : (session.last_action || (session.last_prompt ? "“" + session.last_prompt + "”" : ""))

    implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius * 2
    color: hot
        ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.10)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.04)
    border.width: 1
    border.color: !card.session.seen
        ? Qt.rgba(card.tint.r, card.tint.g, card.tint.b, 0.7)
        : Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.08)
    Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
    Behavior on border.color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }

    // What opening it reveals drops in under the header.
    onOpenChanged: if (open) detailRise.play()

    HoverHandler {
        id: hover
    }

    // Click opens; drag moves the session beside another pane.
    DragGrip {
        anchors.fill: parent
        dnd: card.dnd
        session: card.session
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
        Behavior on color { ColorAnimation { duration: Kirigami.Units.longDuration; easing.type: Easing.InOutQuad } }
    }

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Kirigami.Units.largeSpacing
        anchors.leftMargin: Kirigami.Units.largeSpacing + 4
        anchors.rightMargin: Kirigami.Units.smallSpacing
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
                text: card.session.tab_index ? i18n("Tab %1", card.session.tab_index) : card.session.index
                opacity: 0.5
                font.weight: Font.DemiBold
            }
            PlasmaComponents3.Label {
                text: card.session.project
                font.bold: true
                elide: Text.ElideRight
                Layout.fillWidth: true
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
            ConfirmButton {
                tip: i18n("Close this session")
                needsConfirm: card.busy
                opacity: card.hot || armed ? 1 : 0.35
                onConfirmed: card.deckRoot.closeSession(card.session.sid)
            }
            FoldButton {
                open: card.open
                openText: i18n("Hide the latest message")
                closedText: i18n("Show the latest message and a prompt box")
                onClicked: card.deckRoot.toggleOpen(card.session.sid)
            }
        }

        // What it's doing, and where.
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            visible: card.activity.length > 0 || !!card.session.branch || !!card.session.model

            PlasmaComponents3.Label {
                text: card.activity
                color: card.session.error ? card.tint : Kirigami.Theme.textColor
                opacity: card.session.error ? 1 : 0.75
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Chip {
                text: card.session.branch ? "⎇ " + card.session.branch : ""
                Layout.maximumWidth: Kirigami.Units.gridUnit * 8
            }
            Chip {
                text: card.session.model || ""
                tint: "#d97757"
                strength: 0.16
            }
        }

        Loader {
            id: detailLoader
            Layout.fillWidth: true
            active: card.open
            visible: active
            transform: Rise {
                id: detailRise
                target: detailLoader
                distance: -Kirigami.Units.gridUnit * 0.4
            }
            sourceComponent: SessionDetail {
                session: card.session
                deckRoot: card.deckRoot
            }
        }
    }
}
