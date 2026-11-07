/*
 * The deck's little pixel critter. One per session on the island; its mood
 * mirrors the session:
 *
 *   running   (working)    trots along: legs alternate, body bobs, dust puffs
 *   thinking  (needs you)  eyes up, a thought bubble builds above its head
 *   resting   (your turn)  sits down, eyes closed, z's float up
 *   idle                   stands and blinks now and then
 *   exited                 grey, eyes shut
 *
 * The antenna tip glows in the session's status color.
 * Drawn on a Canvas at whole-pixel scale so it stays crisp on any panel.
 */
import QtQuick

Item {
    id: critter

    property string mood: "idle"
    /// Size of one sprite pixel, in screen pixels.
    property int px: 2
    property color body: "#d97757"
    property color shade: Qt.darker(body, 1.4)
    property color eye: "#1b1112"
    property color accent: "#f2a27f"
    property bool animated: true

    readonly property int cols: 16
    readonly property int rows: 11
    /// Vertical center of the body, for lining up the status light.
    readonly property real bodyCenterY: (mood === "resting" ? 7.5 : 5.5) * px

    width: cols * px
    height: rows * px

    property int frame: 0
    property bool blinking: false

    readonly property var spriteBody: [
        ".....A.....",
        ".....S.....",
        ".BBBBBBBBB.",
        ".BBBBBBBBB.",
        "BBBBBBBBBBB",
        ".BBBBBBBBB.",
        ".BBBBBBBBB."
    ]
    readonly property var spriteRestingTop: [
        "......A....",
        ".....S....."
    ]
    readonly property var legsStand: ["..D.D.D.D..", "..D.D.D.D.."]
    readonly property var legsRunA: ["..D.D.D.D..", "..D...D...."]
    readonly property var legsRunB: ["..D.D.D.D..", "....D...D.."]

    onMoodChanged: { frame = 0; canvas.requestPaint() }
    onPxChanged: canvas.requestPaint()
    onBodyChanged: canvas.requestPaint()
    onAccentChanged: canvas.requestPaint()
    onBlinkingChanged: canvas.requestPaint()

    Timer {
        interval: critter.mood === "running" ? 150 : critter.mood === "thinking" ? 420 : 1600
        running: critter.animated && critter.visible && (critter.mood === "running" || critter.mood === "thinking" || critter.mood === "resting")
        repeat: true
        onTriggered: {
            critter.frame = (critter.frame + 1) % 1000
            canvas.requestPaint()
        }
    }

    // Blink every few seconds while standing or thinking.
    Timer {
        id: blinkTimer
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
        interval: 140
        onTriggered: critter.blinking = false
    }

    Canvas {
        id: canvas
        anchors.fill: parent
        renderTarget: Canvas.Image

        function cell(ctx, color, x, y, w, h) {
            const p = critter.px
            ctx.fillStyle = color
            ctx.fillRect(Math.round(x * p), Math.round(y * p), Math.max(1, Math.round(w * p)), Math.max(1, Math.round(h * p)))
        }

        function rowsAt(ctx, rows, ox, oy, colors) {
            for (let r = 0; r < rows.length; r++) {
                const line = rows[r]
                for (let c = 0; c < line.length; c++) {
                    const ch = line[c]
                    if (ch !== ".") cell(ctx, colors[ch], ox + c, oy + r, 1, 1)
                }
            }
        }

        onPaint: {
            const ctx = getContext("2d")
            ctx.reset()
            const mood = critter.mood
            const exited = mood === "exited"
            const colors = {
                "B": exited ? "#7d6f70" : critter.body.toString(),
                "D": exited ? "#5c5051" : critter.shade.toString(),
                "S": exited ? "#5c5051" : critter.shade.toString(),
                "A": exited ? "#5c5051" : critter.accent.toString()
            }
            const eye = critter.eye.toString()
            const f = critter.frame
            const ox = 1
            let oy = 1

            if (mood === "resting") {
                // Sitting: no legs, body on the ground, antenna drooped.
                oy = 3
                rowsAt(ctx, critter.spriteRestingTop, ox, oy, colors)
                rowsAt(ctx, critter.spriteBody.slice(2), ox, oy + 2, colors)
                // Closed eyes.
                cell(ctx, eye, ox + 3, oy + 3.6, 1.2, 0.4)
                cell(ctx, eye, ox + 7, oy + 3.6, 1.2, 0.4)
                return
            }

            const runB = mood === "running" && f % 2 === 1
            if (runB) oy = 0
            rowsAt(ctx, critter.spriteBody, ox, oy, colors)
            const legs = mood === "running" ? (runB ? critter.legsRunB : critter.legsRunA) : critter.legsStand
            rowsAt(ctx, legs, ox, oy + 7, colors)

            // Eyes.
            const closed = critter.blinking || exited
            if (closed) {
                cell(ctx, eye, ox + 3, oy + 3.6, 1, 0.4)
                cell(ctx, eye, ox + 7, oy + 3.6, 1, 0.4)
            } else if (mood === "running") {
                cell(ctx, eye, ox + 4, oy + 3, 1, 1)       // looking ahead
                cell(ctx, eye, ox + 8, oy + 3, 1, 1)
            } else if (mood === "thinking") {
                cell(ctx, eye, ox + 4, oy + 3, 1, 0.55)    // looking up
                cell(ctx, eye, ox + 8, oy + 3, 1, 0.55)
            } else {
                cell(ctx, eye, ox + 3, oy + 3, 1, 1)
                cell(ctx, eye, ox + 7, oy + 3, 1, 1)
            }

            if (mood === "running" && !runB) {
                // Dust kicked up behind.
                cell(ctx, "rgba(255,255,255,0.35)", 0, oy + 8, 0.7, 0.7)
                cell(ctx, "rgba(255,255,255,0.22)", 0.1, oy + 6.6, 0.5, 0.5)
            }

            if (mood === "thinking") {
                // The thought bubble builds up: one, two, three, hold.
                const n = [0, 1, 2, 3, 3][f % 5]
                const c = critter.accent.toString()
                if (n >= 1) cell(ctx, c, 11.2, 2.4, 0.8, 0.8)
                if (n >= 2) cell(ctx, c, 12.3, 1.1, 1, 1)
                if (n >= 3) cell(ctx, c, 13.4, 0, 1.5, 1.2)
            }
        }
    }

    // Resting: z's drift up from the head.
    Repeater {
        model: critter.mood === "resting" && critter.animated ? 2 : 0
        Text {
            id: zz
            required property int index
            text: "z"
            color: critter.accent
            font.pixelSize: Math.max(6, critter.px * (index === 0 ? 3.2 : 4.2))
            font.bold: true
            opacity: 0
            x: 9 * critter.px
            y: 3 * critter.px

            SequentialAnimation {
                running: true
                loops: Animation.Infinite
                PauseAnimation { duration: zz.index * 1300 }
                ParallelAnimation {
                    NumberAnimation { target: zz; property: "y"; from: 3.5 * critter.px; to: -1.5 * critter.px; duration: 2600; easing.type: Easing.OutSine }
                    NumberAnimation { target: zz; property: "x"; from: 9.5 * critter.px; to: 12.5 * critter.px; duration: 2600; easing.type: Easing.InOutSine }
                    SequentialAnimation {
                        NumberAnimation { target: zz; property: "opacity"; from: 0; to: 0.95; duration: 500 }
                        PauseAnimation { duration: 1300 }
                        NumberAnimation { target: zz; property: "opacity"; from: 0.95; to: 0; duration: 800 }
                    }
                }
                PauseAnimation { duration: (1 - zz.index) * 1300 }
            }
        }
    }
}
