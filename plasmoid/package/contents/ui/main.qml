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
import "logic.js" as Logic

PlasmoidItem {
    id: root

    // ---- State from the daemon -------------------------------------------
    property var deck: ({ running: false, rev: 0, sessions: [], counts: {}, unseen: 0, summary: "" })
    readonly property bool running: deck.running === true
    readonly property var sessions: deck.sessions || []
    readonly property int unseen: deck.unseen || 0
    readonly property string focusedSid: deck.focused_sid || ""
    property real now: Date.now()

    /// Stands in for a session in a delegate that is about to go away. Repeaters
    /// are driven by counts (so delegates survive updates); when the count drops, a
    /// delegate can see its index fall off the end for an instant.
    readonly property var noSession: ({ sid: "", index: 0, status: "exited", seen: true, since: 0, project: "", cwd: "" })
    readonly property var noGroup: ({ key: "", tab: null, tabIndex: 0, sessions: [], top: noSession,
        counts: { working: 0, needs_you: 0, your_turn: 0, exited: 0 }, model: "", projects: [], unseen: 0 })

    /// Sessions grouped by deck tab, in on-screen order (see Logic.groupSessions).
    readonly property var groups: Logic.groupSessions(sessions)

    function urgency(s) {
        return Logic.urgency(s)
    }

    /// What is opened up in the popup: tab groups (by group key) and sessions (by
    /// sid). Everything starts folded; this outlives the popup closing.
    property var openKeys: ({})
    function isOpen(key) {
        return openKeys[key] === true
    }
    function toggleOpen(key) {
        const o = Object.assign({}, openKeys)
        if (o[key]) delete o[key]
        else o[key] = true
        openKeys = o
    }

    /// Popup height in grid units for its first opening, before its content exists
    /// to be measured.
    readonly property real popupUnits: Logic.popupUnits(groups, openKeys)

    /// sid -> the text of Claude's latest message ("" before it has said anything).
    /// Fetched when a session is opened up in the popup or hovered on the island,
    /// and again when it moves on.
    property var replies: ({})
    function fetchReply(sid) {
        run("reply " + shellQuote(sid), (code, out) => {
            if (code !== 0) return
            const text = out.replace(/\s+$/, "")
            if (replies[sid] === text) return
            const r = Object.assign({}, replies)
            r[sid] = text
            replies = r
        })
    }

    /// Type `text` into a session's prompt and submit it; `cb(exitCode, stdout, stderr)`.
    function send(sid, text, cb) {
        run("send " + shellQuote(sid) + " -- " + shellQuote(text), cb)
    }

    /// kitty tab id -> { layout, panes: [{ window, x, y, w, h }] } with x/y/w/h from 0
    /// to 1 inside the tab: the popup draws these as drop zones while dragging.
    property var paneTabs: ({})
    function fetchPanes() {
        run("panes", (code, out) => {
            if (code !== 0) return
            try {
                const m = {}
                for (const t of JSON.parse(out.trim())) m[t.tab] = t
                root.paneTabs = m
            } catch (e) {}
        })
    }

    /// Move a session's pane beside another pane ("left" / "right" / "top" / "bottom"),
    /// or, with no `nextTo`, into a tab of its own.
    function place(sid, nextTo, side) {
        const where = nextTo ? " --next-to " + nextTo + " --side " + side : ""
        run("place " + shellQuote(sid) + where, afterPeek(() => fetchPanes()))
    }

    /// Spread one deck tab's sessions into tabs of their own.
    function untileGroup(group) {
        if (group.tab !== null && group.tab !== undefined) run("untile --tab " + group.tab, () => fetchPanes())
    }

    // ---- The deck window, from the task manager ---------------------------
    property string deckUuid: ""
    property bool deckActive: false
    property bool wasFullscreen: false
    /// When the fullscreen app we are waiting out came up (ms): the digest tells only
    /// of what began waiting since.
    property real fullscreenSince: 0

    // ---- Peek bookkeeping ---------------------------------------------------
    property string peekedSid: ""
    /// A session is being dragged in the popup: no peeking at what the pointer crosses.
    property bool dragging: false

    readonly property string bin: Plasmoid.configuration.binaryPath.length > 0
        ? shellQuote(Plasmoid.configuration.binaryPath)
        : "\"$HOME/.local/bin/claude-deck\""

    Plasmoid.status: !running ? PlasmaCore.Types.HiddenStatus
        : unseen > 0 ? PlasmaCore.Types.NeedsAttentionStatus
        : PlasmaCore.Types.ActiveStatus
    Plasmoid.icon: "claudedeck"
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
            text: i18n("Show Sessions")
            icon.name: "view-list-details-symbolic"
            enabled: root.running
            onTriggered: root.expanded = true
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
            text: i18n("Spread Sessions into Tabs")
            icon.name: "tab-detach-symbolic"
            enabled: root.groups.length < root.sessions.length
            onTriggered: root.run("untile")
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

    /// What the session's critter is doing.
    function moodFor(status) {
        switch (status) {
        case "working": return "running"
        case "needs_you": return "thinking"
        case "your_turn": return "resting"
        default: return "exited"
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
        return Logic.elapsed(now - since)
    }

    // ---- Running the CLI ----------------------------------------------------
    property int nonce: 0
    property var pending: ({})

    function shellQuote(s) {
        return Logic.shellQuote(s)
    }

    /// Run `claude-deck <args>`; `cb(exitCode, stdout, stderr)` when it finishes.
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
            if (cb) cb(data["exit code"], data.stdout || "", data.stderr || "")
        }
    }

    function watch() {
        const since = running ? deck.rev : 0
        run("watch --once --since " + since, (code, out) => {
            const next = Logic.parseWatch(out)
            if (next) {
                const wasRunning = root.running
                // Only a real change is assigned: every assignment re-evaluates the
                // whole island and popup.
                if (Logic.stateChanged(root.deck, next)) root.deck = next
                if (!next.running && wasRunning) {
                    root.expanded = false
                    root.replies = ({})
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
    /// Ends any peek for an action that replaces it (a click, a close, a drop), and
    /// returns the action's callback. If a peek was asked for, an unpeek follows the
    /// action: the `peek` command may still be on its way and land after the click,
    /// and the daemon would then think you're still peeking (so looking at panes
    /// wouldn't mark them seen) until the next click.
    function afterPeek(cb) {
        unpeekTimer.stop()
        const hadPeek = peekedSid.length > 0
        peekedSid = ""
        if (!hadPeek) return cb || null
        return (code, out) => {
            run("unpeek")
            if (cb) cb(code, out)
        }
    }

    function activate(sid) {
        if (sid === focusedSid && deckActive) {
            run("toggle", afterPeek())   // clicking the session you're in hides the deck
        } else {
            run("activate " + shellQuote(sid), afterPeek())
        }
    }

    /// Close one session's pane without raising the deck (the popup stays open).
    /// The popup asks first when Claude is busy, so kitty doesn't have to.
    function closeSession(sid) {
        run("close " + shellQuote(sid) + " --force", peekedSid === sid ? afterPeek() : null)
    }

    /// Close a whole deck tab and every session in it.
    function closeGroup(group) {
        const done = afterPeek()
        if (group.tab !== null && group.tab !== undefined) {
            run("close-tab " + group.tab, done)
        } else {
            for (const s of group.sessions) run("close " + shellQuote(s.sid) + " --force", done)
        }
    }

    function requestPeek(sid) {
        if (!Plasmoid.configuration.peekOnHover || deckActive || dragging) {
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

    /// Scroll on the island: focus the next/previous session.
    function cycle(step) {
        const i = Logic.cycleIndex(sessions, focusedSid, step)
        if (i >= 0) run("activate " + shellQuote(sessions[i].sid))
    }

    // The drop map's panes: fresh when the popup opens and when a drag starts.
    onExpandedChanged: if (root.expanded) fetchPanes()

    /// Forget opened-up and fetched state of sessions that are gone.
    onSessionsChanged: {
        const live = {}
        for (const s of sessions) {
            live[s.sid] = true
            live[Logic.groupKey(s)] = true
        }
        const prune = m => {
            const out = {}
            let dropped = false
            for (const k in m) {
                if (live[k]) out[k] = m[k]
                else dropped = true
            }
            return dropped ? out : m
        }
        openKeys = prune(openKeys)
        replies = prune(replies)
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
        // The deck itself going fullscreen is not being away from it.
        const fs = !active && tasks.data(tasks.activeTask, TaskManager.AbstractTasksModel.IsFullScreen) === true
        if (fs && !wasFullscreen) fullscreenSince = Date.now()
        if (wasFullscreen && !fs && running && unseen > 0) {
            // Back from a game: Plasma held the notifications, so say it now, for what
            // began waiting meanwhile (not for what has been red since this morning).
            run("digest --since " + Math.floor(fullscreenSince))
            if (Plasmoid.configuration.expandOnReturn) root.expanded = true
        }
        wasFullscreen = fs
    }
}
