import qs
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.stance
import QtQuick
import Quickshell

// stance for the end4-pC bar (an illogical-impulse fork). Follows the bar's
// button style (classic circle or Material pill). One per bar = one per
// screen; each bar owns the confirm card for its own screen.
Item {
    id: root

    property color contentColor: Appearance.colors.colOnLayer2
    readonly property bool material: Config.options.bar.cornerStyle === 3 || Config.options.bar.cornerStyle === 4

    implicitWidth: buttonLoader.implicitWidth
    implicitHeight: buttonLoader.implicitHeight

    Loader {
        id: buttonLoader
        anchors.centerIn: parent
        sourceComponent: root.material ? materialButton : classicButton
    }

    Component {
        id: materialButton
        UtilButton {
            iconText: "monitor"
            isActive: stancePopup.open
            onClicked: stancePopup.open = !stancePopup.open
        }
    }

    Component {
        id: classicButton
        CircleUtilButton {
            onClicked: stancePopup.open = !stancePopup.open
            MaterialSymbol {
                horizontalAlignment: Qt.AlignHCenter
                fill: stancePopup.open ? 1 : 0
                text: "monitor"
                iconSize: Appearance.font.pixelSize.large
                color: root.contentColor
            }
        }
    }

    StanceService {
        id: stanceService
    }

    StanceTheme {
        id: stanceTheme
        background: Appearance.m3colors.m3surfaceContainerLow
        card: Appearance.colors.colLayer2
        surface: Appearance.colors.colLayer3
        surfaceHover: Appearance.colors.colLayer3Hover
        border: Appearance.colors.colLayer0Border
        text: Appearance.colors.colOnLayer2
        textDim: Appearance.colors.colSubtext
        accent: Appearance.colors.colPrimary
        accentText: Appearance.colors.colOnPrimary
        danger: Appearance.colors.colError
        dangerText: Appearance.m3colors.m3onError
        radius: Appearance.rounding.normal
        chipRadius: Appearance.rounding.small
        fontFamily: Appearance.font.family.main
        fontSize: Appearance.font.pixelSize.small
        // The shell's own iconMaterial family may not be installed; Rounded
        // ships with its dependencies.
        iconFont: "Material Symbols Rounded"
        animMs: Appearance.animation.elementMoveFast.duration
    }

    StancePopup {
        id: stancePopup
        service: stanceService
        theme: stanceTheme
        anchorItem: root
        bottom: Config.options.bar.bottom
        barHeight: Appearance.sizes.barHeight
    }

    ConfirmOverlay {
        service: stanceService
        theme: stanceTheme
        enabled: root.visible
        screen: root.QsWindow.window ? root.QsWindow.window.screen : null
    }
}
