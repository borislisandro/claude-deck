/*
 * Claude Deck — the panel island.
 * Talks to the claude-deck daemon only through the claude-deck CLI: a
 * `watch --once` long-poll for state, one-shot commands for actions.
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami
import org.kde.taskmanager as TaskManager

PlasmoidItem {
    id: root

    // ---- State from the daemon -------------------------------------------
    property var deck: ({ running: false, rev: 0, sessions: [], counts: {}, unseen: 0, summary: "" })
    readonly property bool running: deck.running === true
    readonly property var sessions: deck.sessions || []
    readonly property int unseen: deck.unseen || 0
    readonly property string focusedSid: deck.focused_sid || ""
    property real now: Date.now()
    /// sid -> last lines of its pane, fetched while hovering or while the popup is open.
    property var tails: ({})

    // ---- The deck window, from the task manager ---------------------------
    property string deckUuid: ""
    property bool deckActive: false
    property bool wasFullscreen: false

    // ---- Peek bookkeeping ---------------------------------------------------
    property string peekedSid: ""

    readonly property string bin: Plasmoid.configuration.binaryPath.length > 0
        ? shellQuote(Plasmoid.configuration.binaryPath)
        : "\"$HOME/.local/bin/claude-deck\""

    Plasmoid.status: !running ? PlasmaCore.Types.HiddenStatus
        : unseen > 0 ? PlasmaCore.Types.NeedsAttentionStatus
        : PlasmaCore.Types.ActiveStatus
    Plasmoid.icon: "utilities-terminal"
    Plasmoid.title: "Claude Deck"
    toolTipMainText: "Claude Deck"
    toolTipSubText: running ? deck.summary : i18n("Not running")
    preferredRepresentation: compactRepresentation
    switchWidth: Kirigami.Units.gridUnit * 10
    switchHeight: Kirigami.Units.gridUnit * 10

    compactRepresentation: CompactRepresentation { deckRoot: root }
    fullRepresentation: FullRepresentation { deckRoot: root }

    Plasmoid.contextualActions: [
        PlasmaCore.Action {
            text: root.running ? i18n("New Session…") : i18n("Open Claude Deck")
            icon.name: "list-add-symbolic"
            onTriggered: root.running ? root.run("new --pick") : root.run("toggle")
        },
        PlasmaCore.Action {
            text: i18n("Jump to Next Waiting")
            icon.name: "go-jump-symbolic"
            enabled: root.unseen > 0
            onTriggered: root.run("next-waiting --raise")
        },
        PlasmaCore.Action {
            text: i18n("Tile All Sessions")
            icon.name: "view-grid-symbolic"
            enabled: root.sessions.length > 1
            onTriggered: root.run("tile")
        },
        PlasmaCore.Action {
            text: i18n("Restore Last Workspace")
            icon.name: "document-revert-symbolic"
            onTriggered: root.running ? root.run("restore") : root.run("toggle")
        },
        PlasmaCore.Action {
            text: i18n("Quit Claude Deck")
            icon.name: "application-exit-symbolic"
            enabled: root.running
            onTriggered: root.run("quit")
        }
    ]

    // ---- Colors -------------------------------------------------------------
    function statusColor(status, seen) {
        switch (status) {
        case "working": return Plasmoid.configuration.colorWorking
        case "needs_you": return Plasmoid.configuration.colorNeedsYou
        case "your_turn": return seen ? Qt.darker(Plasmoid.configuration.colorYourTurn, 1.45) : Plasmoid.configuration.colorYourTurn
        default: return Plasmoid.configuration.colorExited
        }
    }

    function statusLabel(status) {
        switch (status) {
        case "working": return i18n("Working")
        case "needs_you": return i18n("Needs you")
        case "your_turn": return i18n("Your turn")
        default: return i18n("Exited")
        }
    }

    function elapsed(since) {
        const s = Math.max(0, Math.floor((now - since) / 1000))
        if (s < 60) return s + "s"
        if (s < 3600) return Math.floor(s / 60) + "m"
        const m = Math.floor((s % 3600) / 60)
        return Math.floor(s / 3600) + "h" + (m < 10 ? "0" : "") + m
    }

    // ---- Running the CLI ----------------------------------------------------
    property int nonce: 0
    property var pending: ({})

    function shellQuote(s) {
        return "'" + String(s).replace(/'/g, "'\\''") + "'"
    }

    /// Run `claude-deck <args>`; `cb(exitCode, stdout)` when it finishes.
    function run(args, cb) {
        nonce += 1
        // The executable engine keys sources by command line; the comment keeps each unique.
        const cmd = bin + " " + args + " # " + nonce
        pending[cmd] = cb || null
        exec.connectSource(cmd)
    }

    P5Support.DataSource {
        id: exec
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => {
            disconnectSource(source)
            const cb = root.pending[source]
            delete root.pending[source]
            if (cb) cb(data["exit code"], data.stdout || "")
        }
    }

    function watch() {
        const since = running ? deck.rev : 0
        run("watch --once --since " + since, (code, out) => {
            let next = null
            try {
                const lines = out.trim().split("\n")
                next = JSON.parse(lines[lines.length - 1])
            } catch (e) {
                next = null
            }
            if (next) {
                const wasRunning = root.running
                root.deck = next
                if (!next.running && wasRunning) {
                    root.expanded = false
                    root.tails = ({})
                }
                watch()
            } else {
                retry.start()
            }
        })
    }

    Timer {
        id: retry
        interval: 2000
        onTriggered: root.watch()
    }

    Timer {
        interval: 1000
        running: root.running
        repeat: true
        onTriggered: root.now = Date.now()
    }

    Component.onCompleted: watch()

    // ---- Actions used by the representations -------------------------------
    function activate(sid) {
        unpeekTimer.stop()
        peekedSid = ""
        if (sid === focusedSid && deckActive) {
            run("toggle")   // clicking the session you're in hides the deck
        } else {
            run("activate " + shellQuote(sid))
        }
    }

    function requestPeek(sid) {
        if (!Plasmoid.configuration.peekOnHover || deckActive) {
            return
        }
        unpeekTimer.stop()
        if (peekedSid === sid) {
            return
        }
        peekedSid = sid
        run("peek " + shellQuote(sid) + (deckUuid.length > 0 ? " --uuid " + shellQuote(deckUuid) : ""))
    }

    function requestUnpeek() {
        unpeekTimer.restart()
    }

    Timer {
        // Moving between icons shouldn't flash back to the original tab in between.
        id: unpeekTimer
        interval: 180
        onTriggered: {
            if (root.peekedSid.length > 0) {
                root.peekedSid = ""
                root.run("unpeek")
            }
        }
    }

    function fetchTail(sid, lines) {
        run("tail " + shellQuote(sid) + " --lines " + (lines || 6), (code, out) => {
            if (code !== 0) return
            const t = Object.assign({}, root.tails)
            t[sid] = out.replace(/\s+$/, "")
            root.tails = t
        })
    }

    /// Scroll on the island: focus the next/previous session.
    function cycle(step) {
        if (sessions.length === 0) return
        let i = sessions.findIndex(s => s.sid === focusedSid)
        i = i < 0 ? 0 : (i + step + sessions.length) % sessions.length
        run("activate " + shellQuote(sessions[i].sid))
    }

    onExpandedChanged: {
        if (root.expanded) {
            for (const s of sessions) fetchTail(s.sid, 6)
        }
    }

    Timer {
        interval: 2500
        repeat: true
        running: root.expanded && root.running
        onTriggered: {
            for (const s of root.sessions) root.fetchTail(s.sid, 6)
        }
    }

    // ---- Finding the deck window; noticing the end of a fullscreen game ----
    TaskManager.TasksModel {
        id: tasks
        groupMode: TaskManager.TasksModel.GroupDisabled
        filterByVirtualDesktop: false
        filterByScreen: false
        filterByActivity: false
        filterNotMinimized: false
        onCountChanged: scanTimer.restart()
        onDataChanged: scanTimer.restart()
        onActiveTaskChanged: scanTimer.restart()
    }

    Timer {
        id: scanTimer
        interval: 50
        onTriggered: root.scanTasks()
    }

    function scanTasks() {
        let uuid = ""
        let active = false
        for (let i = 0; i < tasks.count; i++) {
            const idx = tasks.makeModelIndex(i)
            const app = String(tasks.data(idx, TaskManager.AbstractTasksModel.AppId) || "")
            if (app.indexOf("claude-deck") >= 0) {
                const ids = tasks.data(idx, TaskManager.AbstractTasksModel.WinIdList) || []
                if (ids.length > 0) uuid = String(ids[0])
                active = tasks.data(idx, TaskManager.AbstractTasksModel.IsActive) === true
                break
            }
        }
        deckUuid = uuid
        deckActive = active
        const fs = tasks.data(tasks.activeTask, TaskManager.AbstractTasksModel.IsFullScreen) === true
        if (wasFullscreen && !fs && running && unseen > 0) {
            // Back from a game: Plasma held the notifications, so say it now.
            run("digest")
            if (Plasmoid.configuration.expandOnReturn) root.expanded = true
        }
        wasFullscreen = fs
    }
}
