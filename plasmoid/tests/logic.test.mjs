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
        s("d", "your_turn", { seen: false }),
        s("e", "needs_you", { seen: true }),
        s("f", "needs_you", { seen: false }),
    ].sort((x, y) => L.urgency(x) - L.urgency(y)).map(x => x.sid)
    assert.deepEqual(order, ["f", "e", "d", "c", "b", "a"])
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
    assert.deepEqual(plain(api.counts), { working: 1, needs_you: 0, your_turn: 1, exited: 0 })
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

test("popupUnits stays within bounds and follows opening", () => {
    const one = { key: "a", sessions: [{ sid: "s1" }] }
    const three = { key: "b", sessions: [{ sid: "s2" }, { sid: "s3" }, { sid: "s4" }] }
    assert.equal(L.popupUnits([], {}), 8, "minimum")
    assert.equal(L.popupUnits([one, three], {}), 5.5 + 4 + 2.6, "everything folded")
    assert.equal(L.popupUnits([one, three], { b: true }), 5.5 + 4 + 2.6 + 6)
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
