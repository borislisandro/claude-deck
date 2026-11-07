import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kcmutils as KCM

KCM.SimpleKCM {
    property alias cfg_binaryPath: binaryPath.text
    property alias cfg_collapseAfter: collapseAfter.value
    property alias cfg_showElapsed: showElapsed.checked
    property alias cfg_spinWhileWorking: spinWhileWorking.checked
    property alias cfg_claudeColoredGlyph: claudeColoredGlyph.checked
    property alias cfg_ownBackground: ownBackground.checked
    property alias cfg_peekOnHover: peekOnHover.checked
    property alias cfg_expandOnReturn: expandOnReturn.checked
    property alias cfg_showThumbnail: showThumbnail.checked
    property alias cfg_colorWorking: colorWorking.text
    property alias cfg_colorNeedsYou: colorNeedsYou.text
    property alias cfg_colorYourTurn: colorYourTurn.text
    property alias cfg_colorExited: colorExited.text

    Kirigami.FormLayout {
        QQC2.CheckBox {
            id: peekOnHover
            Kirigami.FormData.label: i18n("Behavior:")
            text: i18n("Hovering a session shows its pane")
        }
        QQC2.CheckBox {
            id: expandOnReturn
            text: i18n("After a fullscreen app closes, open the popup if sessions are waiting")
        }
        QQC2.CheckBox {
            id: showThumbnail
            text: i18n("Live thumbnail of the deck in the popup")
        }

        Item { Kirigami.FormData.isSection: true }

        QQC2.CheckBox {
            id: showElapsed
            Kirigami.FormData.label: i18n("Island:")
            text: i18n("Show how long a session has waited")
        }
        QQC2.CheckBox {
            id: spinWhileWorking
            text: i18n("Spin the spark while Claude works")
        }
        QQC2.CheckBox {
            id: claudeColoredGlyph
            text: i18n("Claude-orange spark")
        }
        QQC2.CheckBox {
            id: ownBackground
            text: i18n("Draw the island background (off: let Panel Colorizer style it)")
        }
        QQC2.SpinBox {
            id: collapseAfter
            Kirigami.FormData.label: i18n("Collapse to counts above:")
            from: 1
            to: 20
        }

        Item { Kirigami.FormData.isSection: true }

        QQC2.TextField { id: colorWorking; Kirigami.FormData.label: i18n("Working:") }
        QQC2.TextField { id: colorNeedsYou; Kirigami.FormData.label: i18n("Needs you:") }
        QQC2.TextField { id: colorYourTurn; Kirigami.FormData.label: i18n("Your turn:") }
        QQC2.TextField { id: colorExited; Kirigami.FormData.label: i18n("Exited:") }

        Item { Kirigami.FormData.isSection: true }

        QQC2.TextField {
            id: binaryPath
            Kirigami.FormData.label: i18n("claude-deck binary:")
            placeholderText: "~/.local/bin/claude-deck"
        }
    }
}
