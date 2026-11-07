/*
 * An eight-ray spark, drawn in QML so it stays crisp at any panel size and
 * can be tinted and spun.
 */
import QtQuick

Item {
    id: spark

    property real size: 16
    property color color: "#d97757"
    property bool spinning: false

    width: size
    height: size

    Item {
        id: rays
        anchors.fill: parent

        Repeater {
            model: 4
            Rectangle {
                required property int index
                // Alternate long and short rays for an organic, hand-drawn look.
                readonly property real len: spark.size * (index % 2 === 0 ? 1.0 : 0.82)
                width: Math.max(1.6, spark.size * 0.15)
                height: len
                radius: width / 2
                x: (spark.width - width) / 2
                y: (spark.height - height) / 2
                rotation: index * 45
                antialiasing: true
                color: spark.color
            }
        }

        RotationAnimator on rotation {
            running: spark.spinning
            from: 0
            to: 360
            duration: 7000
            loops: Animation.Infinite
        }
    }

    // Settle back upright when work stops.
    onSpinningChanged: if (!spinning) rays.rotation = 0
}
