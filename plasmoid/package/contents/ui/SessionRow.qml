/*
 * One session inside a tab group in the popup: a single line, which opens (the
 * arrow) onto Claude's latest message and a prompt box.
 * A click opens the session (hovering does nothing but highlight); the close button
 * asks first when Claude is busy.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: row

    required property var session
    required property var deckRoot
    /// The popup's drag controller.
    property var dnd: null
    /// Show the project name (when the tab mixes projects) instead of only the activity.
    property bool showProject: false
    /// Lit from elsewhere: its pane is hovered on the tab's map.
    property bool lit: false

    readonly property color tint: deckRoot.statusColor(session.status, session.seen)
    // Closing asks first while Claude is at work, or has work running in the background.
    readonly property bool busy: session.status === "working" || session.status === "needs_you" || session.status === "background"
    readonly property bool hot: hover.hovered
    readonly property bool open: deckRoot.isOpen(session.sid)

    implicitHeight: col.implicitHeight + Kirigami.Units.smallSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: hot || lit || open
        ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, hot || lit ? 0.12 : 0.05)
        : "transparent"
    Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

    // What opening it reveals drops in under the line.
    onOpenChanged: if (open) detailRise.play()

    HoverHandler {
        id: hover
    }
    DragGrip {
        anchors.fill: parent
        dnd: row.dnd
        session: row.session
        onTapped: row.deckRoot.activate(row.session.sid)
    }

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Kirigami.Units.smallSpacing
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.rightMargin: 2
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            StatusDot {
                size: 7
                status: row.session.status
                seen: row.session.seen
                color: row.tint
                waited: row.deckRoot.now - row.session.since
            }
            PlasmaComponents3.Label {
                text: row.session.index
                opacity: 0.5
                font.weight: Font.DemiBold
                font.features: { "tnum": 1 }
                Layout.minimumWidth: Kirigami.Units.gridUnit * 0.9
            }
            PlasmaComponents3.Label {
                visible: row.showProject
                text: row.session.project
                font.bold: true
                elide: Text.ElideRight
                Layout.maximumWidth: Kirigami.Units.gridUnit * 7
            }
            PlasmaComponents3.Label {
                text: row.session.error ? "⚠ " + row.session.error
                    : (row.session.last_action || row.session.last_prompt || i18n("Ready"))
                color: row.session.error ? row.tint : Kirigami.Theme.textColor
                opacity: 0.75
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Chip {
                text: row.session.model || ""
                tint: "#d97757"
                strength: 0.16
            }
            PlasmaComponents3.Label {
                text: row.deckRoot.statusLabel(row.session.status)
                color: row.tint
                font.weight: Font.DemiBold
            }
            PlasmaComponents3.Label {
                text: row.deckRoot.elapsed(row.session.since)
                opacity: 0.55
                font.features: { "tnum": 1 }
                Layout.minimumWidth: Kirigami.Units.gridUnit * 1.6
                horizontalAlignment: Text.AlignRight
            }
            ConfirmButton {
                tip: i18n("Close this session")
                needsConfirm: row.busy
                opacity: row.hot || armed ? 1 : 0.35
                onConfirmed: row.deckRoot.closeSession(row.session.sid)
            }
            FoldButton {
                open: row.open
                openText: i18n("Hide the latest message")
                closedText: i18n("Show the latest message and a prompt box")
                opacity: row.hot || row.open ? 1 : 0.6
                onClicked: row.deckRoot.toggleOpen(row.session.sid)
            }
        }

        Loader {
            id: detailLoader
            transform: Rise {
                id: detailRise
                target: detailLoader
                distance: -Kirigami.Units.gridUnit * 0.4
            }
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.gridUnit
            Layout.rightMargin: Kirigami.Units.smallSpacing
            Layout.bottomMargin: Kirigami.Units.smallSpacing
            active: row.open
            visible: active
            sourceComponent: SessionDetail {
                session: row.session
                deckRoot: row.deckRoot
            }
        }
    }
}
