/*
 * The deck's little pixel critter. One per session on the island; its mood
 * mirrors the session:
 *
 *   running   (working)    trots: striding legs, swinging arms, bob, trailing
 *                          antenna, speed streaks, dust at each footfall
 *   thinking  (needs you)  hand on chin, eyes glancing up left and right, a
 *                          thought bubble growing into a cloud with "…" inside
 *   resting   (your turn)  lies down asleep: breathing, curved closed eyes,
 *                          flopped antenna, pixel Z's drifting up
 *   idle                   stands, blinks, glances around
 *   exited                 grey, eyes shut
 *
 * The antenna tip glows in the session's status color. Everything is drawn on a
 * Canvas at whole-pixel scale so it stays crisp on any panel. The body takes
 * columns 1-11 of the 16; the space above it holds the thought bubble (left) and
 * the Z's (right).
 */
import QtQuick

Item {
    id: critter

    property string mood: "idle"
    /// Size of one sprite pixel, in screen pixels.
    property int px: 2
    property color body: "#d97757"
    property color eye: "#1b1112"
    property color accent: "#f2a27f"
    property bool animated: true

    readonly property int cols: 16
    readonly property int rows: 11
    /// Where the body ends on the right, and its vertical center, for placing the light.
    readonly property real bodyRight: 12 * px
    readonly property real bodyCenterY: (mood === "resting" ? 8.6 : 5.5) * px

    width: cols * px
    height: rows * px

    property int frame: 0
    property bool blinking: false

    onMoodChanged: { frame = 0; canvas.requestPaint() }
    onPxChanged: canvas.requestPaint()
    onBodyChanged: canvas.requestPaint()
    onAccentChanged: canvas.requestPaint()
    onBlinkingChanged: canvas.requestPaint()

    Timer {
        interval: critter.mood === "running" ? 115 : critter.mood === "thinking" ? 170 : critter.mood === "resting" ? 120 : 900
        running: critter.animated && critter.visible && critter.mood !== "exited"
        repeat: true
        onTriggered: {
            critter.frame = (critter.frame + 1) % 100000
            canvas.requestPaint()
        }
    }

    Timer {
        interval: 2600 + Math.random() * 2400
        running: critter.animated && critter.visible && (critter.mood === "idle" || critter.mood === "thinking")
        repeat: true
        onTriggered: {
            critter.blinking = true
            unblink.restart()
            interval = 2600 + Math.random() * 2400
        }
    }
    Timer {
        id: unblink
        interval: 130
        onTriggered: critter.blinking = false
    }

    Canvas {
        id: canvas
        anchors.fill: parent
        renderTarget: Canvas.Image

        property var ctx: null

        readonly property var bodyRows: [
            ".HHBBBBBBB.",
            ".BBBBBBBBB.",
            "BBBBBBBBBBB",
            ".BBBBBBBBB.",
            ".MMMMMMMMM."
        ]

        function rgba(c, a) {
            return "rgba(" + Math.round(c.r * 255) + "," + Math.round(c.g * 255) + "," + Math.round(c.b * 255) + "," + a + ")"
        }

        /// A rectangle in sprite cells.
        function cell(color, x, y, w, h) {
            const p = critter.px
            ctx.fillStyle = color
            ctx.fillRect(Math.round(x * p), Math.round(y * p), Math.max(1, Math.round(w * p)), Math.max(1, Math.round(h * p)))
        }

        function clearCell(x, y) {
            const p = critter.px
            ctx.clearRect(Math.round(x * p), Math.round(y * p), p, p)
        }

        /// Rows of characters -> cells; '.' is empty.
        function sprite(rows, ox, oy, pal) {
            for (let r = 0; r < rows.length; r++) {
                for (let c = 0; c < rows[r].length; c++) {
                    const ch = rows[r][c]
                    if (ch !== "." && pal[ch]) cell(pal[ch], ox + c, oy + r, 1, 1)
                }
            }
        }

        /// Eyes; `oy` is the sprite top (eyes sit 3 rows down), `dx` shifts the gaze.
        function eyesOpen(oy, dx, upper) {
            const e = critter.eye.toString()
            cell(e, 4 + dx, oy + 3, 1, upper ? 0.6 : 1)
            cell(e, 8 + dx, oy + 3, 1, upper ? 0.6 : 1)
        }

        function eyesClosed(oy, curved) {
            const e = critter.eye.toString()
            for (const x of [4, 8]) {
                if (curved) {
                    // ‿ : the ends a little higher than the middle.
                    cell(e, x - 0.5, oy + 3.2, 0.5, 0.5)
                    cell(e, x, oy + 3.55, 1, 0.5)
                    cell(e, x + 1, oy + 3.2, 0.5, 0.5)
                } else {
                    cell(e, x, oy + 3.55, 1, 0.45)
                }
            }
        }

        /// A crisp pixel Z, `s` screen pixels wide, at screen coordinates.
        function zee(x, y, s, color) {
            ctx.fillStyle = color
            x = Math.round(x)
            y = Math.round(y)
            ctx.fillRect(x, y, s, 1)
            ctx.fillRect(x, y + s - 1, s, 1)
            for (let i = 1; i < s - 1; i++) ctx.fillRect(x + s - 1 - i, y + i, 1, 1)
        }

        onPaint: {
            ctx = getContext("2d")
            ctx.reset()
            const grey = critter.mood === "exited"
            const pal = {
                "B": grey ? "#7d6f70" : critter.body.toString(),
                "H": grey ? "#8e8081" : Qt.lighter(critter.body, 1.2).toString(),
                "M": grey ? "#6b5f60" : Qt.darker(critter.body, 1.15).toString(),
                "D": grey ? "#564b4c" : Qt.darker(critter.body, 1.5).toString(),
                "S": grey ? "#564b4c" : Qt.darker(critter.body, 1.5).toString(),
                "A": grey ? "#564b4c" : critter.accent.toString()
            }
            const f = critter.frame
            switch (critter.mood) {
            case "running": drawRunning(pal, f); break
            case "thinking": drawThinking(pal, f); break
            case "resting": drawResting(pal, f); break
            default: drawStanding(pal, f, grey)
            }
        }

        function drawStanding(pal, f, grey) {
            const oy = 1
            sprite([".....A.....", ".....S....."], 1, oy, pal)
            sprite(bodyRows, 1, oy + 2, pal)
            sprite(["..D.D.D.D..", "..D.D.D.D.."], 1, oy + 7, pal)
            if (grey || critter.blinking) {
                eyesClosed(oy, false)
            } else {
                // Now and then a glance to one side.
                const beat = Math.floor(f / 4) % 6
                eyesOpen(oy, beat === 5 ? 1 : (beat === 2 ? -1 : 0), false)
            }
        }

        function drawRunning(pal, f) {
            const phase = f % 4                 // 0 stride, 1 airborne, 2 stride, 3 airborne
            const air = phase % 2 === 1
            const oy = air ? 0 : 1
            // Antenna trails behind the run, swinging a little.
            sprite([phase === 2 ? "...A......." : "....A......", ".....S....."], 1, oy, pal)
            sprite(bodyRows, 1, oy + 2, pal)
            // Arms swing opposite each other: one a row up, the other a row down.
            clearCell(1, oy + 4)
            clearCell(11, oy + 4)
            cell(pal.B, 1, oy + (phase === 0 ? 3 : phase === 2 ? 5 : 4), 1, 1)
            cell(pal.B, 11, oy + (phase === 2 ? 3 : phase === 0 ? 5 : 4), 1, 1)
            // Legs: diagonal strides on the ground, tucked in the air.
            const legs = phase === 0 ? ["..D.D.D.D..", ".D.D...D.D."]
                : phase === 2 ? ["..D.D.D.D..", "...D.D.D..."]
                : ["..D.D.D.D.."]
            sprite(legs, 1, oy + 7, pal)
            eyesOpen(oy, 1, false)              // eyes on the road ahead
            // Speed streaks behind, and dust where a foot lands.
            const streak = "rgba(255,255,255," + (air ? 0.30 : 0.18) + ")"
            cell(streak, 0, oy + (phase < 2 ? 3 : 5), 0.8, 0.5)
            cell(streak, 0.2, oy + (phase < 2 ? 5.5 : 3.5), 0.5, 0.5)
            if (!air) {
                cell("rgba(255,255,255,0.32)", 0.3, oy + 8.4, 0.7, 0.6)
                cell("rgba(255,255,255,0.18)", 0, oy + 7.6, 0.5, 0.5)
            }
        }

        function drawThinking(pal, f) {
            const oy = 1
            // The antenna tip pulses.
            const tipOn = Math.floor(f / 3) % 2 === 0
            cell(tipOn ? pal.A : rgba(critter.accent, 0.4), 6, oy, 1, 1)
            sprite([".....S....."], 1, oy + 1, pal)
            sprite(bodyRows, 1, oy + 2, pal)
            // Hand on the chin: the right arm lifted up to the face.
            clearCell(11, oy + 4)
            cell(pal.B, 11, oy + 3, 1, 1)
            cell(pal.M, 10.5, oy + 2.5, 0.5, 0.5)
            sprite(["..D.D.D.D..", "..D.D.D.D.."], 1, oy + 7, pal)
            if (critter.blinking) {
                eyesClosed(oy, false)
            } else {
                // Gazing up, to the right, then back.
                eyesOpen(oy, Math.floor(f / 14) % 2 === 0 ? 1 : 0, true)
            }
            // The thought bubble rises on the left of the head (the right is the
            // light's): two dots, then a cloud with "…" filling up inside.
            const t = f % 34
            const cloud = "rgba(243,234,217,0.95)"
            if (t >= 2) cell(cloud, 4.4, 2.3, 0.5, 0.5)
            if (t >= 5) cell(cloud, 3.5, 1.6, 0.7, 0.7)
            if (t >= 8) {
                cell(cloud, 0.5, 0, 2, 0.5)
                cell(cloud, 0, 0.5, 3, 1.5)
                cell(cloud, 0.5, 2, 2, 0.5)
                const n = 1 + Math.floor((t - 8) / 3) % 3
                for (let i = 0; i < 3; i++) {
                    cell(i < n ? critter.accent.toString() : rgba(critter.accent, 0.25), 0.35 + i * 0.8, 1.0, 0.5, 0.5)
                }
            }
        }

        function drawResting(pal, f) {
            // Lying flat: legs tucked under, belly on the ground, slow breaths.
            const inhale = Math.floor(f / 12) % 2 === 1
            const top = inhale ? 6 : 7          // the back rises with each breath
            // Antenna flopped over to the side.
            cell(pal.S, 7, top - 1, 1, 1)
            cell(pal.A, 8, top - 0.6, 1, 1)
            const rows = [".HHBBBBBBB."]
            for (let y = top + 1; y < 8; y++) rows.push(".BBBBBBBBB.")
            rows.push("BBBBBBBBBBB")
            rows.push(".MMMMMMMMM.")
            sprite(rows, 1, top, pal)
            // Eyes on the row below the highlight, whatever the breath.
            eyesClosed(top - 2, true)
            // Z's float up from the head, growing and fading; two, half a cycle apart.
            const p = critter.px
            for (const offset of [0, 14]) {
                const q = ((f + offset) % 28) / 28
                const x = (9 + q * 5) * p
                const y = (top - 1.5 - q * 5.5) * p
                const s = Math.round(3 + q * 3)
                const a = q < 0.15 ? q / 0.15 : (q > 0.7 ? (1 - q) / 0.3 : 1)
                zee(x, y, s, rgba(critter.accent, (0.95 * a).toFixed(2)))
            }
        }
    }
}
