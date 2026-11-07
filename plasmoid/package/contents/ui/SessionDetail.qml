/*
 * A session opened up in the popup: Claude's latest message in full (scroll to
 * read it all; nothing at all until Claude has said something), a box to prompt
 * the session from here, and its actions.
 * The message is fetched when this opens and again whenever the session moves on;
 * nothing polls.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: detail

    required property var session
    required property var deckRoot

    readonly property string reply: deckRoot.replies[session.sid] || ""
    readonly property bool asking: session.status === "needs_you"
    readonly property bool exited: session.status === "exited"
    readonly property bool busy: session.status === "working" || asking
    /// Changes whenever Claude may have written something new.
    readonly property string stamp: [session.sid, session.status, session.since,
        session.last_action || "", session.last_prompt || ""].join("|")

    property bool sending: false
    property string sendError: ""

    spacing: Kirigami.Units.smallSpacing

    onStampChanged: deckRoot.fetchReply(session.sid)
    Component.onCompleted: deckRoot.fetchReply(session.sid)

    function send() {
        const text = prompt.text.trim()
        if (text.length === 0 || sending || asking || exited) return
        sending = true
        sendError = ""
        deckRoot.send(session.sid, text, (code, out, err) => {
            detail.sending = false
            if (code === 0) {
                prompt.text = ""
            } else {
                detail.sendError = (err || "").replace(/^claude-deck:\s*/, "").trim() || i18n("Could not send")
                errorTimer.restart()
            }
        })
    }

    Timer {
        id: errorTimer
        interval: 6000
        onTriggered: detail.sendError = ""
    }

    // Claude's latest message, rendered as the Markdown it is.
    Rectangle {
        visible: detail.reply.length > 0
        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(replyText.implicitHeight + Kirigami.Units.smallSpacing * 2,
                                         Kirigami.Units.gridUnit * 14)
        radius: Kirigami.Units.cornerRadius
        color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.05)
        border.width: 1
        border.color: Qt.rgba(Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.08)

        PlasmaComponents3.ScrollView {
            id: replyScroll
            anchors.fill: parent
            anchors.margins: Kirigami.Units.smallSpacing
            PlasmaComponents3.ScrollBar.horizontal.policy: PlasmaComponents3.ScrollBar.AlwaysOff

            TextEdit {
                id: replyText
                width: replyScroll.availableWidth
                rightPadding: Kirigami.Units.smallSpacing
                text: detail.reply
                textFormat: TextEdit.MarkdownText
                wrapMode: TextEdit.Wrap
                readOnly: true
                selectByMouse: true
                color: Kirigami.Theme.textColor
                selectionColor: Kirigami.Theme.highlightColor
                selectedTextColor: Kirigami.Theme.highlightedTextColor
                font: Kirigami.Theme.defaultFont
                onLinkActivated: link => Qt.openUrlExternally(link)

                HoverHandler {
                    cursorShape: replyText.hoveredLink.length > 0 ? Qt.PointingHandCursor : Qt.IBeamCursor
                }
            }
        }
    }

    // Prompt the session from here. Enter sends, Shift+Enter is a new line.
    RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
        visible: !detail.exited

        PlasmaComponents3.TextArea {
            id: prompt
            Layout.fillWidth: true
            Layout.maximumHeight: Kirigami.Units.gridUnit * 6
            enabled: !detail.asking && !detail.sending
            wrapMode: TextEdit.Wrap
            placeholderText: detail.asking ? i18n("Claude is asking you something: open the session to answer")
                : detail.session.status === "working" ? i18n("Queue a message for #%1…", detail.session.index)
                : i18n("Message #%1 %2…", detail.session.index, detail.session.project)
            Keys.onReturnPressed: event => {
                if (event.modifiers & Qt.ShiftModifier) {
                    event.accepted = false
                } else {
                    detail.send()
                }
            }
            Keys.onEnterPressed: event => detail.send()
        }
        PlasmaComponents3.ToolButton {
            Layout.alignment: Qt.AlignBottom
            icon.name: "document-send-symbolic"
            display: PlasmaComponents3.AbstractButton.IconOnly
            text: i18n("Send")
            enabled: prompt.text.trim().length > 0 && !detail.asking && !detail.sending
            onClicked: detail.send()
            PlasmaComponents3.ToolTip.text: i18n("Send (Enter) · Shift+Enter for a new line")
            PlasmaComponents3.ToolTip.visible: hovered
        }
    }

    PlasmaComponents3.Label {
        visible: detail.sendError.length > 0
        text: "⚠ " + detail.sendError
        color: Kirigami.Theme.negativeTextColor
        font: Kirigami.Theme.smallFont
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 0
        visible: detail.busy || !detail.session.seen

        PlasmaComponents3.ToolButton {
            visible: detail.busy
            icon.name: "media-playback-stop-symbolic"
            text: i18n("Interrupt")
            onClicked: detail.deckRoot.run("interrupt " + detail.deckRoot.shellQuote(detail.session.sid))
        }
        PlasmaComponents3.ToolButton {
            visible: !detail.session.seen
            icon.name: "checkmark-symbolic"
            text: i18n("Mark seen")
            onClicked: detail.deckRoot.run("ack " + detail.deckRoot.shellQuote(detail.session.sid))
        }
        Item { Layout.fillWidth: true }
    }
}
