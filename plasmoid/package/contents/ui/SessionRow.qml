/*
 * One session inside a tab group in the popup: a single compact line.
 * Hover peeks, click opens, the close button asks first when Claude is busy.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

Rectangle {
    id: row

    required property var session
    required property var deckRoot
    /// Show the project name (when the tab mixes projects) instead of only the activity.
    property bool showProject: false

    readonly property color tint: deckRoot.statusColor(session.status, session.seen)
    readonly property bool busy: session.status === "working" || session.status === "needs_you"
    readonly property bool hot: hover.hovered

    implicitHeight: line.implicitHeight + Kirigami.Units.smallSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: hot ? Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.12) : "transparent"
    Behavior on color { ColorAnimation { duration: 100 } }

    HoverHandler {
        id: hover
        onHoveredChanged: hovered ? row.deckRoot.requestPeek(row.session.sid) : row.deckRoot.requestUnpeek()
    }
    TapHandler {
        onTapped: row.deckRoot.activate(row.session.sid)
    }

    RowLayout {
        id: line
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.rightMargin: 2
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
        PlasmaComponents3.ToolButton {
            visible: row.busy
            icon.name: "media-playback-stop-symbolic"
            display: PlasmaComponents3.AbstractButton.IconOnly
            text: i18n("Interrupt")
            opacity: row.hot ? 1 : 0.35
            onClicked: row.deckRoot.run("interrupt " + row.deckRoot.shellQuote(row.session.sid))
            PlasmaComponents3.ToolTip.text: text
            PlasmaComponents3.ToolTip.visible: hovered
        }
        ConfirmButton {
            tip: i18n("Close this session")
            needsConfirm: row.busy
            opacity: row.hot || armed ? 1 : 0.35
            onConfirmed: row.deckRoot.closeSession(row.session.sid)
        }
    }
}
