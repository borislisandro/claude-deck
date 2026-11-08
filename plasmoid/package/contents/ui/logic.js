.pragma library
// The widget's pure logic: no QML types, no Plasma, no clock. The QML files call
// these; plasmoid/tests/logic.test.mjs runs them under node.

/// Lower is more urgent: what the island and the groups show first.
function urgency(s) {
    if (s.status === "needs_you") return s.seen ? 1 : 0
    if (s.status === "your_turn") return s.seen ? 4 : 2
    if (s.status === "working") return 3
    // Busy without you: below working, above a your-turn you've already seen.
    if (s.status === "background") return 3.5
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
        g.counts = { working: 0, needs_you: 0, your_turn: 0, background: 0, exited: 0 }
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

/// "2d 3h", "2h 10m", "35m", "<1m" for a duration in milliseconds (as the deck's
/// status line writes it).
function span(ms) {
    const s = Math.max(0, Math.floor(ms / 1000))
    const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60)
    if (d > 0) return d + "d " + h + "h"
    if (h > 0) return h + "h " + m + "m"
    if (m > 0) return m + "m"
    return "<1m"
}

/// The 5-hour and weekly limits, for the popup: [{ key, label, known, left (percent
/// left, whole), resetsIn (ms), level }]. The level is "ok" from half left up,
/// "half" under 50% left, "low" under 20% and "out" once nothing is left. `usage` is the
/// daemon's { five_hour: { used, resets_at (s) }, seven_day: … }; a window whose reset
/// time has passed is full again. Empty while no window is known at all.
function usageMeters(usage, nowMs) {
    const kinds = [["five_hour", "5-hour"], ["seven_day", "Weekly"]]
    const u = usage || {}
    if (!kinds.some(k => u[k[0]])) return []
    return kinds.map(([key, label]) => {
        const w = u[key]
        if (!w || typeof w.used !== "number") return { key, label, known: false, left: 0, resetsIn: 0, level: "ok" }
        const resetsIn = Math.max(0, (w.resets_at || 0) * 1000 - nowMs)
        const left = resetsIn > 0 ? Math.max(0, Math.min(100, Math.round(100 - w.used))) : 100
        const level = left <= 0 ? "out" : left < 20 ? "low" : left < 50 ? "half" : "ok"
        return { key, label, known: true, left, resetsIn, level }
    })
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

/// What the popup header's critter does. It belongs to no session, so it sums up
/// the whole deck in moods of its own (`counts` as the daemon sends them):
///   exited    the deck isn't running
///   idle      no live sessions
///   waving    someone needs you (it wins over everything else)
///   juggling  every session is working (or waits on its background work)
///   typing    some are working, the rest wait for your next prompt
///   coffee    every session waits for your next prompt
function deckMood(running, counts) {
    if (!running) return "exited"
    const c = counts || {}
    // Background work keeps a session busy without you, like working does.
    const working = (c.working || 0) + (c.background || 0), needsYou = c.needs_you || 0, yourTurn = c.your_turn || 0
    if (working + needsYou + yourTurn === 0) return "idle"
    if (needsYou > 0) return "waving"
    if (yourTurn === 0) return "juggling"
    if (working > 0) return "typing"
    return "coffee"
}

/// Popup height in grid units, estimated from what it will show (`open`: the
/// opened-up group keys and sids). Only the first opening uses it: after that the
/// popup measures its content.
function popupUnits(groups, open) {
    let u = 5.5
    for (const g of groups) {
        const single = g.sessions.length === 1
        u += single ? 4 : 2.6
        // Opened: the map of its panes, then a line per session.
        if (!single && open[g.key]) u += 6 + 2 * g.sessions.length
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

/// A tab's height over its width (how the deck draws it), or `fallback` when kitty
/// hasn't said.
function tabAspect(group, paneTabs, fallback) {
    const t = group && group.tab !== null && group.tab !== undefined ? paneTabs[group.tab] : null
    return t && t.aspect > 0 ? t.aspect : fallback
}

// ---- A tab's splits as a tree -------------------------------------------------
// A node is a window id (a pane) or { h, parts: [{ size, node }] }: its parts side
// by side (h) or stacked, each with its share of the room (the shares add up to 1).

/// kitty's splits tree ({ h, bias, one, two }, window ids at the leaves) as a node.
function fromKittyTree(t) {
    if (t === null || t === undefined) return null
    if (typeof t === "number") return t
    const one = fromKittyTree(t.one)
    const two = fromKittyTree(t.two)
    if (one === null) return two
    if (two === null) return one
    const bias = t.bias > 0 && t.bias < 1 ? t.bias : 0.5
    return { h: !!t.h, parts: [{ size: bias, node: one }, { size: 1 - bias, node: two }] }
}

/// A node rebuilt from pane rectangles (for layouts other than splits): the tab
/// cut into columns, or failing that rows, and each of those likewise. Null if the
/// panes can't be cut up that way.
function treeFromPanes(panes) {
    if (!panes || panes.length === 0) return null
    if (panes.length === 1) return panes[0].window
    for (const h of [true, false]) {
        const at = p => h ? p.x : p.y
        const len = p => h ? p.w : p.h
        const sorted = panes.slice().sort((a, b) => at(a) - at(b))
        const runs = []
        let cur = [], end = -Infinity
        for (const p of sorted) {
            // Panes sit a border apart, so a cut is wherever the next starts at or
            // past (nearly) the end of everything before it.
            if (cur.length > 0 && at(p) >= end - 0.01) {
                runs.push(cur)
                cur = []
                end = -Infinity
            }
            cur.push(p)
            end = Math.max(end, at(p) + len(p))
        }
        runs.push(cur)
        if (runs.length < 2) continue
        const parts = []
        for (const r of runs) {
            const node = treeFromPanes(r)
            if (node === null) return null
            const lo = Math.min(...r.map(at))
            parts.push({ size: Math.max(...r.map(p => at(p) + len(p))) - lo, node: node })
        }
        return { h: h, parts: shares(parts) }
    }
    return null
}

function shares(parts) {
    const total = parts.reduce((t, p) => t + p.size, 0)
    return parts.map(p => ({ size: total > 0 ? p.size / total : 1 / parts.length, node: p.node }))
}

/// The node with a pane taken out: as in kitty, what was split with it takes its room.
function withoutWindow(node, window) {
    if (node === null || node === undefined) return null
    if (typeof node === "number") return node === window ? null : node
    const parts = node.parts
        .map(p => ({ size: p.size, node: withoutWindow(p.node, window) }))
        .filter(p => p.node !== null)
    if (parts.length === 0) return null
    if (parts.length === 1) return parts[0].node
    return { h: node.h, parts: shares(parts) }
}

/// A node laid out in the unit square: [{ window, x, y, w, h }].
function layoutTree(node, x, y, w, h, out) {
    out = out || []
    if (node === null || node === undefined) return out
    if (typeof node === "number") {
        out.push({ window: node, x: x, y: y, w: w, h: h })
        return out
    }
    let at = node.h ? x : y
    for (const p of node.parts) {
        if (node.h) layoutTree(p.node, at, y, w * p.size, h, out)
        else layoutTree(p.node, x, at, w, h * p.size, out)
        at += (node.h ? w : h) * p.size
    }
    return out
}

/// A tab's panes once `window` has moved out of it (`window` not in the tab: as
/// they are). From kitty's tree when it sent one, else rebuilt from the panes; if
/// neither works, the others stay put around the hole.
function panesWithout(group, paneTabs, window) {
    const panes = panesFor(group, paneTabs)
    if (!panes.some(p => p.window === window)) return panes
    const t = group && group.tab !== null && group.tab !== undefined ? paneTabs[group.tab] : null
    let tree = t && t.tree !== undefined ? fromKittyTree(t.tree) : null
    // A tree that disagrees with the panes (stale) is no use.
    if (tree !== null && layoutTree(tree, 0, 0, 1, 1).length !== panes.length) tree = null
    if (tree === null) tree = treeFromPanes(panes)
    if (tree === null) return panes.filter(p => p.window !== window)
    return layoutTree(withoutWindow(tree, window), 0, 0, 1, 1)
}
