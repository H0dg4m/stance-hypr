import qs
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.stance
import QtQuick
import QtQuick.Layouts
import Quickshell

// stance for end-4's illogical-impulse (ii) bar. One per bar, i.e. one per
// screen: each bar owns the confirm card for its own screen.
Item {
    id: root

    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight
    Layout.alignment: Qt.AlignVCenter

    CircleUtilButton {
        id: button
        anchors.centerIn: parent
        onClicked: stancePopup.open = !stancePopup.open

        MaterialSymbol {
            horizontalAlignment: Qt.AlignHCenter
            fill: stancePopup.open ? 1 : 0
            text: "monitor"
            iconSize: Appearance.font.pixelSize.large
            color: Appearance.colors.colOnLayer2
        }
    }

    StanceService {
        id: stanceService
        themeTarget: stanceTheme
        defaultTheme: "shell"
    }

    StanceTheme {
        id: stanceTheme
        // The shell's own iconMaterial family may not be installed; Rounded
        // ships with its dependencies.
        // What the shell looks like; a theme picked in the settings wins over it.
        shell: ({
            background: Appearance.m3colors.m3surfaceContainerLow,
            card: Appearance.colors.colLayer2,
            surface: Appearance.colors.colLayer3,
            surfaceHover: Appearance.colors.colLayer3Hover,
            border: Appearance.colors.colLayer0Border,
            text: Appearance.colors.colOnLayer2,
            textDim: Appearance.colors.colSubtext,
            accent: Appearance.colors.colPrimary,
            accentText: Appearance.colors.colOnPrimary,
            danger: Appearance.colors.colError,
            dangerText: Appearance.m3colors.m3onError,
            radius: Appearance.rounding.normal,
            chipRadius: Appearance.rounding.small,
            fontFamily: Appearance.font.family.main,
            fontSize: Appearance.font.pixelSize.small,
            iconFont: "Material Symbols Rounded",
            animMs: Appearance.animation.elementMoveFast.duration
        })
    }

    StancePopup {
        id: stancePopup
        service: stanceService
        theme: stanceTheme
        anchorItem: button
        // ii has no stance clipboard shelf.
        hasClipShelf: false
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
