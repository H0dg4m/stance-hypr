import qs
import qs.modules.common
import qs.modules.stance
import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io

// stance clipboard shelf for end4-pC: one instance for the whole shell
// (the bar button exists once per screen, an IPC target must not).
// Install: symlink next to StanceUtilButton.qml in modules/ii/bar/ and add
//   PanelLoader { component: StanceClipShelf {} }
// to panelFamilies/IllogicalImpulseFamily.qml. Bind a key to
//   qs -c end4-pC ipc call stanceclip toggle
Scope {
    id: root

    StanceService {
        id: stanceService
    }

    StanceClipService {
        id: clipService
        bin: stanceService.bin
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
        iconFont: "Material Symbols Rounded"
        animMs: Appearance.animation.elementMoveFast.duration
    }

    readonly property var focusedScreen: {
        const name = Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : "";
        const all = Quickshell.screens;
        for (let i = 0; i < all.length; i++)
            if (all[i].name === name) return all[i];
        return all.length > 0 ? all[0] : null;
    }

    StanceClip {
        id: shelf
        clip: clipService
        theme: stanceTheme
        screen: root.focusedScreen
        onSettingsRequested: {
            shelf.open = false;
            settingsWindow.open = true;
        }
    }

    StanceSettingsWindow {
        id: settingsWindow
        service: stanceService
        theme: stanceTheme
        screen: root.focusedScreen
    }

    IpcHandler {
        target: "stanceclip"
        function toggle(): void { shelf.open = !shelf.open; }
        function open(): void { shelf.open = true; }
        function close(): void { shelf.open = false; }
    }
}
