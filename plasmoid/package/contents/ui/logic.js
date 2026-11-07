.pragma library
// The widget's pure logic: no QML types, no Plasma, no clock. The QML files call
// these; plasmoid/tests/logic.test.mjs runs them under node.

/// Lower is more urgent: what the island and the groups show first.
function urgency(s) {
    if (s.status === "needs_you") return s.seen ? 1 : 0
    if (s.status === "your_turn") return s.seen ? 4 : 2
    if (s.status === "working") return 3
    return 5
}

/// Sessions grouped by deck tab, in on-screen order (the daemon sends them sorted).
/// Each group: { key, tab, tabIndex, sessions, top (most urgent), counts, model,
/// projects, unseen }. A session whose tab isn't known yet is a group of its own.
function groupSessions(sessions) {
    const out = []
    const byKey = {}
    for (const s of sessions || []) {
        const key = groupKey(s)
        let g = byKey[key]
        if (!g) {
            g = { key: key, tab: s.tab, tabIndex: s.tab_index || (out.length + 1), sessions: [] }
            byKey[key] = g
            out.push(g)
        }
        g.sessions.push(s)
    }
    for (const g of out) {
        g.top = g.sessions.slice().sort((a, b) => urgency(a) - urgency(b) || a.since - b.since)[0]
        g.counts = { working: 0, needs_you: 0, your_turn: 0, exited: 0 }
        for (const s of g.sessions) g.counts[s.status] = (g.counts[s.status] || 0) + 1
        const models = [...new Set(g.sessions.map(s => s.model || ""))]
        // A model for the whole tab only makes sense when every session agrees.
        g.model = models.length === 1 ? models[0] : ""
        g.projects = [...new Set(g.sessions.map(s => s.project))]
        g.unseen = g.sessions.filter(s => !s.seen).length
    }
    return out
}

/// The key of the group (deck tab) a session is in.
function groupKey(s) {
    return (s.tab !== null && s.tab !== undefined) ? "t" + s.tab : "s" + s.sid
}

/// The last `n` non-empty lines of a text.
function lastLines(text, n) {
    const lines = String(text || "").split("\n").filter(l => l.trim().length > 0)
    return lines.slice(Math.max(0, lines.length - n)).join("\n")
}

/// "42s", "4m", "1h05" for a duration in milliseconds.
function elapsed(ms) {
    const s = Math.max(0, Math.floor(ms / 1000))
    if (s < 60) return s + "s"
    if (s < 3600) return Math.floor(s / 60) + "m"
    const m = Math.floor((s % 3600) / 60)
    return Math.floor(s / 3600) + "h" + (m < 10 ? "0" : "") + m
}

/// One shell word, whatever it contains.
function shellQuote(s) {
    return "'" + String(s).replace(/'/g, "'\\''") + "'"
}

/// How the island draws: one critter per session, one per tab, or counts.
function islandMode(sessionCount, groupCount, collapseAfter) {
    if (sessionCount <= collapseAfter) return "sessions"
    if (groupCount <= collapseAfter) return "tabs"
    return "counts"
}

/// Popup height in grid units, estimated from what it will show (`open`: the
/// opened-up group keys and sids). Only the first opening uses it: after that the
/// popup measures its content.
function popupUnits(groups, open) {
    let u = 5.5
    for (const g of groups) {
        const single = g.sessions.length === 1
        u += single ? 4 : 2.6
        if (!single && open[g.key]) u += 2 * g.sessions.length
        for (const s of g.sessions) {
            if (open[s.sid] && (single || open[g.key])) u += 12
        }
    }
    return Math.min(44, Math.max(8, u))
}

/// Index of the session to focus when scrolling `step` (+1 / -1) on the island,
/// or -1 with no sessions. With nothing focused, scrolling down starts at the first
/// session and scrolling up at the last.
function cycleIndex(sessions, focusedSid, step) {
    const n = sessions.length
    if (n === 0) return -1
    const i = sessions.findIndex(s => s.sid === focusedSid)
    if (i < 0) return step > 0 ? 0 : n - 1
    return ((i + step) % n + n) % n
}

/// The state in `claude-deck watch --once` output (its last line), or null.
function parseWatch(out) {
    const lines = String(out || "").trim().split("\n")
    try {
        const v = JSON.parse(lines[lines.length - 1])
        return v && typeof v === "object" && "running" in v ? v : null
    } catch (e) {
        return null
    }
}

/// Whether a new state differs from the shown one. The long poll also answers
/// after 25 s with nothing new; redrawing then would only restart animations.
function stateChanged(prev, next) {
    return !prev || next.running !== prev.running || next.rev !== prev.rev
}

/// What dropping a dragged session at (nx, ny), from 0 to 1 inside a tab's map,
/// would do: { self: true } over its own pane, { window, side, pane } over another
/// (side: whichever edge of that pane is nearest), or null between panes.
function dropHit(panes, nx, ny, draggedWindow) {
    for (const pane of panes) {
        if (nx < pane.x || ny < pane.y || nx > pane.x + pane.w || ny > pane.y + pane.h) continue
        if (pane.window === draggedWindow) return { self: true }
        const u = (nx - pane.x) / pane.w
        const v = (ny - pane.y) / pane.h
        const d = { left: u, right: 1 - u, top: v, bottom: 1 - v }
        let side = "left"
        for (const k of ["right", "top", "bottom"]) if (d[k] < d[side]) side = k
        return { window: pane.window, side: side, pane: pane }
    }
    return null
}

/// A tab's panes for the drop map: kitty's layout when known, otherwise one column
/// per session that has a pane.
function panesFor(group, paneTabs) {
    if (!group) return []
    const t = group.tab !== null && group.tab !== undefined ? paneTabs[group.tab] : null
    if (t && t.panes.length > 0) return t.panes
    const ws = group.sessions.filter(s => !!s.kitty_window)
    return ws.map((s, i) => ({ window: s.kitty_window, x: i / ws.length, y: 0, w: 1 / ws.length, h: 1 }))
}
