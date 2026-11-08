// Tests for the widget's logic (contents/ui/logic.js). Run: node --test plasmoid/tests/
import { test } from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import vm from "node:vm"

// logic.js is a QML JavaScript resource: plain functions after a ".pragma library"
// line, which is QML syntax, not JavaScript.
const src = readFileSync(new URL("../package/contents/ui/logic.js", import.meta.url), "utf8")
const L = {}
vm.createContext(L)
vm.runInContext(src.replace(/^\.pragma library$/m, ""), L)
// Values made inside the vm context have that context's prototypes; compare plain copies.
const plain = v => JSON.parse(JSON.stringify(v))

const s = (sid, status, extra = {}) => ({ sid, status, seen: true, since: 0, project: "p", ...extra })

test("urgency: unseen needs-you first, exited last", () => {
    const order = [
        s("a", "exited"),
        s("b", "your_turn", { seen: true }),
        s("c", "working"),
        s("g", "background"),
        s("d", "your_turn", { seen: false }),
        s("e", "needs_you", { seen: true }),
        s("f", "needs_you", { seen: false }),
    ].sort((x, y) => L.urgency(x) - L.urgency(y)).map(x => x.sid)
    assert.deepEqual(order, ["f", "e", "d", "c", "g", "b", "a"], "background: below working, above a seen your-turn")
})

test("groupSessions keeps on-screen order and picks each tab's most urgent session", () => {
    const groups = L.groupSessions([
        s("a", "working", { tab: 10, tab_index: 1, model: "Opus 5.5", project: "api" }),
        s("b", "your_turn", { tab: 10, tab_index: 1, seen: false, since: 5, model: "Opus 5.5", project: "api" }),
        s("c", "needs_you", { tab: 20, tab_index: 2, model: "Sonnet 5.5", project: "web" }),
        s("d", "working", { tab: 20, tab_index: 2, model: "Opus 5.5", project: "docs" }),
        s("e", "working", { tab: null }),
    ])
    assert.deepEqual(plain(groups.map(g => g.key)), ["t10", "t20", "se"])
    const [api, mixed, loose] = groups
    assert.equal(api.top.sid, "b")
    assert.equal(api.model, "Opus 5.5", "every session agrees")
    assert.equal(api.unseen, 1)
    assert.deepEqual(plain(api.counts), { working: 1, needs_you: 0, your_turn: 1, background: 0, exited: 0 })
    assert.equal(mixed.top.sid, "c")
    assert.equal(mixed.model, "", "mixed models: none for the tab")
    assert.deepEqual(plain(mixed.projects), ["web", "docs"])
    assert.equal(loose.tabIndex, 3, "a session without a known tab still gets a number")
    assert.deepEqual(plain(L.groupSessions([])), [])
    assert.deepEqual(plain(L.groupSessions(undefined)), [])
})

test("groupSessions breaks urgency ties by who waited longest", () => {
    const [g] = L.groupSessions([
        s("late", "your_turn", { tab: 1, seen: false, since: 900 }),
        s("early", "your_turn", { tab: 1, seen: false, since: 100 }),
    ])
    assert.equal(g.top.sid, "early")
})

test("elapsed", () => {
    assert.equal(L.elapsed(0), "0s")
    assert.equal(L.elapsed(59_999), "59s")
    assert.equal(L.elapsed(60_000), "1m")
    assert.equal(L.elapsed(3_599_000), "59m")
    assert.equal(L.elapsed(3_600_000 + 5 * 60_000), "1h05")
    assert.equal(L.elapsed(-5000), "0s", "a clock running backwards")
})

test("shellQuote survives the shell", () => {
    assert.equal(L.shellQuote("abc"), "'abc'")
    assert.equal(L.shellQuote("it's"), "'it'\\''s'")
    assert.equal(L.shellQuote("$(rm -rf ~)"), "'$(rm -rf ~)'")
    assert.equal(L.shellQuote(42), "'42'")
})

test("islandMode collapses sessions, then tabs", () => {
    assert.equal(L.islandMode(5, 5, 5), "sessions")
    assert.equal(L.islandMode(6, 2, 5), "tabs")
    assert.equal(L.islandMode(9, 6, 5), "counts")
    assert.equal(L.islandMode(0, 0, 5), "sessions")
})

test("deckMood sums up the deck for the header critter", () => {
    assert.equal(L.deckMood(false, { working: 2 }), "exited", "deck not running")
    assert.equal(L.deckMood(true, {}), "idle", "no sessions")
    assert.equal(L.deckMood(true, { exited: 3 }), "idle", "only exited sessions")
    assert.equal(L.deckMood(true, { working: 2, needs_you: 1, your_turn: 1 }), "waving", "needs you wins")
    assert.equal(L.deckMood(true, { working: 3, exited: 1 }), "juggling", "every live session working")
    assert.equal(L.deckMood(true, { working: 1, your_turn: 2 }), "typing", "some working")
    assert.equal(L.deckMood(true, { your_turn: 2, exited: 1 }), "coffee", "everyone waits on you")
    assert.equal(L.deckMood(true, { background: 2 }), "juggling", "background work is busy without you")
    assert.equal(L.deckMood(true, { background: 1, your_turn: 1 }), "typing")
    assert.equal(L.deckMood(true, { background: 1, needs_you: 1 }), "waving")
})

test("popupUnits stays within bounds and follows opening", () => {
    const one = { key: "a", sessions: [{ sid: "s1" }] }
    const three = { key: "b", sessions: [{ sid: "s2" }, { sid: "s3" }, { sid: "s4" }] }
    assert.equal(L.popupUnits([], {}), 8, "minimum")
    assert.equal(L.popupUnits([one, three], {}), 5.5 + 4 + 2.6, "everything folded")
    assert.equal(L.popupUnits([one, three], { b: true }), 5.5 + 4 + 2.6 + 6 + 6, "its map and rows")
    assert.equal(L.popupUnits([one, three], { s1: true, s3: true }), 5.5 + 4 + 12 + 2.6, "a row opens only inside an open tab")
    assert.equal(L.popupUnits(Array(20).fill(one), { s1: true }), 44, "maximum")
})

test("cycleIndex wraps both ways and starts sensibly", () => {
    const ss = [s("a"), s("b"), s("c")]
    assert.equal(L.cycleIndex(ss, "a", 1), 1)
    assert.equal(L.cycleIndex(ss, "c", 1), 0)
    assert.equal(L.cycleIndex(ss, "a", -1), 2)
    assert.equal(L.cycleIndex(ss, "", 1), 0, "nothing focused, scrolling down: the first")
    assert.equal(L.cycleIndex(ss, "", -1), 2, "nothing focused, scrolling up: the last")
    assert.equal(L.cycleIndex([], "a", 1), -1)
})

test("parseWatch takes the last line and rejects garbage", () => {
    assert.deepEqual(plain(L.parseWatch('{"running":false}\n{"running":true,"rev":5}\n')), { running: true, rev: 5 })
    assert.equal(L.parseWatch(""), null)
    assert.equal(L.parseWatch("claude-deck: not found"), null)
    assert.equal(L.parseWatch("[1,2]"), null)
    assert.equal(L.parseWatch("null"), null)
    assert.equal(L.parseWatch(undefined), null)
})

test("stateChanged ignores a long poll that timed out with nothing new", () => {
    const a = { running: true, rev: 7 }
    assert.equal(L.stateChanged(a, { running: true, rev: 7 }), false)
    assert.equal(L.stateChanged(a, { running: true, rev: 8 }), true)
    assert.equal(L.stateChanged(a, { running: false, rev: 0 }), true)
    assert.equal(L.stateChanged(null, a), true)
})

test("dropHit picks the nearest edge of the pane under the pointer", () => {
    const panes = [
        { window: 1, x: 0, y: 0, w: 0.5, h: 1 },
        { window: 2, x: 0.5, y: 0, w: 0.5, h: 1 },
    ]
    assert.equal(L.dropHit(panes, 0.05, 0.5, 9).side, "left")
    assert.equal(L.dropHit(panes, 0.45, 0.5, 9).side, "right")
    assert.equal(L.dropHit(panes, 0.25, 0.05, 9).side, "top")
    const hit = L.dropHit(panes, 0.25, 0.95, 9)
    assert.equal(hit.side, "bottom")
    assert.equal(hit.window, 1)
    assert.equal(L.dropHit(panes, 0.75, 0.5, 9).window, 2)
    assert.deepEqual(plain(L.dropHit(panes, 0.2, 0.5, 1)), { self: true }, "over its own pane")
    assert.equal(L.dropHit(panes, 1.5, 0.5, 9), null, "outside")
    assert.equal(L.dropHit([], 0.5, 0.5, 9), null)
})

test("panesFor prefers kitty's layout, else one column per session", () => {
    const group = { tab: 10, sessions: [s("a", "working", { kitty_window: 1 }), s("b", "working", { kitty_window: 2 }), s("c", "working")] }
    const fromKitty = [{ window: 1, x: 0, y: 0, w: 1, h: 0.5 }, { window: 2, x: 0, y: 0.5, w: 1, h: 0.5 }]
    assert.equal(L.panesFor(group, { 10: { panes: fromKitty } }), fromKitty)
    const guessed = L.panesFor(group, {})
    assert.deepEqual(plain(guessed.map(p => [p.window, p.x, p.w])), [[1, 0, 0.5], [2, 0.5, 0.5]])
    assert.deepEqual(plain(L.panesFor(null, {})), [])
    assert.equal(L.panesFor({ ...group, tab: null }, { 10: { panes: fromKitty } }).length, 2, "no tab: guessed")
})

const round = panes => plain(panes).map(p => [p.window, ...[p.x, p.y, p.w, p.h].map(v => Math.round(v * 1000) / 1000)])

test("panesWithout: kitty's tree closes up around the pane that moves", () => {
    // 1 | (2 over 3): 2 leaves, 3 takes the whole right column.
    const group = { tab: 10, sessions: [] }
    const tab = {
        tree: { h: true, bias: 0.5, one: 1, two: { h: false, bias: 0.5, one: 2, two: 3 } },
        panes: [
            { window: 1, x: 0, y: 0, w: 0.5, h: 1 },
            { window: 2, x: 0.5, y: 0, w: 0.5, h: 0.5 },
            { window: 3, x: 0.5, y: 0.5, w: 0.5, h: 0.5 },
        ],
    }
    assert.deepEqual(round(L.panesWithout(group, { 10: tab }, 2)), [[1, 0, 0, 0.5, 1], [3, 0.5, 0, 0.5, 1]])
    // 1 leaves: the right column (its sibling) takes the whole tab.
    assert.deepEqual(round(L.panesWithout(group, { 10: tab }, 1)), [[2, 0, 0, 1, 0.5], [3, 0, 0.5, 1, 0.5]])
    assert.equal(L.panesWithout(group, { 10: tab }, 99), tab.panes, "not in the tab: as it is")
    assert.deepEqual(plain(L.panesWithout(group, { 10: { panes: [{ window: 1, x: 0, y: 0, w: 1, h: 1 }] } }, 1)), [], "alone in its tab")
})

test("panesWithout rebuilds the cuts from the panes when kitty sent no tree", () => {
    // Three columns a border apart (as kitty measures them); the middle one leaves.
    const group = { tab: 10, sessions: [] }
    const panes = [
        { window: 1, x: 0, y: 0, w: 0.33, h: 1 },
        { window: 2, x: 0.335, y: 0, w: 0.33, h: 1 },
        { window: 3, x: 0.67, y: 0, w: 0.33, h: 1 },
    ]
    const out = round(L.panesWithout(group, { 10: { panes } }, 2))
    assert.deepEqual(out.map(p => p[0]), [1, 3])
    assert.ok(Math.abs(out[0][3] - 0.5) < 0.01 && Math.abs(out[1][1] - 0.5) < 0.01, "the other two share the room")
    // Stacked rows.
    const rows = [{ window: 1, x: 0, y: 0, w: 1, h: 0.5 }, { window: 2, x: 0, y: 0.5, w: 1, h: 0.5 }]
    assert.deepEqual(round(L.panesWithout(group, { 10: { panes: rows } }, 1)), [[2, 0, 0, 1, 1]])
})

test("treeFromPanes gives up on a pinwheel; panesWithout then leaves a hole", () => {
    const pinwheel = [
        { window: 1, x: 0, y: 0, w: 0.66, h: 0.33 },
        { window: 2, x: 0.66, y: 0, w: 0.34, h: 0.66 },
        { window: 3, x: 0.33, y: 0.66, w: 0.67, h: 0.34 },
        { window: 4, x: 0, y: 0.33, w: 0.33, h: 0.67 },
        { window: 5, x: 0.33, y: 0.33, w: 0.33, h: 0.33 },
    ]
    assert.equal(L.treeFromPanes(pinwheel), null)
    assert.equal(L.panesWithout({ tab: 1, sessions: [] }, { 1: { panes: pinwheel } }, 5).length, 4)
})

test("fromKittyTree drops empty halves and odd biases", () => {
    assert.equal(L.fromKittyTree({ h: true, bias: 0.5, one: 4, two: null }), 4)
    assert.deepEqual(plain(L.fromKittyTree({ h: false, bias: 7, one: 1, two: 2 })),
        { h: false, parts: [{ size: 0.5, node: 1 }, { size: 0.5, node: 2 }] })
})

test("tabAspect: kitty's when known", () => {
    assert.equal(L.tabAspect({ tab: 1 }, { 1: { aspect: 0.4 } }, 0.5), 0.4)
    assert.equal(L.tabAspect({ tab: 2 }, { 1: { aspect: 0.4 } }, 0.5), 0.5)
    assert.equal(L.tabAspect(null, {}, 0.5), 0.5)
})

test("span reads like the status line", () => {
    assert.equal(L.span(0), "<1m")
    assert.equal(L.span(35 * 60e3), "35m")
    assert.equal(L.span((2 * 3600 + 10 * 60) * 1e3), "2h 10m")
    assert.equal(L.span((2 * 86400 + 3 * 3600 + 59) * 1e3), "2d 3h")
    assert.equal(L.span(-5), "<1m")
})

test("usageMeters shows what is left of each limit", () => {
    const now = 1_000_000_000_000
    assert.deepEqual(plain(L.usageMeters({}, now)), [], "nothing known: nothing shown")
    assert.deepEqual(plain(L.usageMeters(undefined, now)), [])
    const m = plain(L.usageMeters({
        five_hour: { used: 23.4, resets_at: now / 1000 + 7800 },
        seven_day: { used: 85, resets_at: now / 1000 + 86400 },
    }, now))
    assert.deepEqual(m.map(x => [x.label, x.left, x.level]), [["5-hour", 77, "ok"], ["Weekly", 15, "low"]])
    assert.equal(m[0].resetsIn, 7800e3)
    // Green from half left, yellow under half, red under a fifth, and used up at 0.
    const at = used => plain(L.usageMeters({ five_hour: { used, resets_at: now / 1000 + 60 } }, now))[0].level
    assert.deepEqual([0, 50, 50.6, 80, 80.6, 99.4, 100].map(at), ["ok", "ok", "half", "half", "low", "low", "out"])
    // Only the weekly known: the 5-hour still has its place, marked unknown.
    const one = plain(L.usageMeters({ seven_day: { used: 100, resets_at: now / 1000 + 60 } }, now))
    assert.deepEqual(one.map(x => [x.known, x.left, x.level]), [[false, 0, "ok"], [true, 0, "out"]])
    // Past its reset time, a window is full again until Claude says otherwise.
    const back = plain(L.usageMeters({ five_hour: { used: 100, resets_at: now / 1000 - 1 } }, now))
    assert.deepEqual([back[0].left, back[0].level, back[0].resetsIn], [100, "ok", 0])
})
