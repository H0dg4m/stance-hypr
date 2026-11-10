//@ pragma UseQApplication
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland

// Standalone stance shell for any bar (Waybar, ...):
//   qs -p /usr/share/stance/quickshell          (keep it running)
//   qs -p /usr/share/stance/quickshell ipc call stance toggle
// Quickshell users can embed the components instead (see README).
ShellRoot {
    id: shell

    StanceService {
        id: stanceService
    }

    StanceTheme {
        id: stanceTheme
    }

    // One confirm card per screen.
    Variants {
        model: Quickshell.screens
        delegate: Component {
            Item {
                required property var modelData
                id: perScreen
                ConfirmOverlay {
                    service: stanceService
                    screen: perScreen.modelData
                    theme: stanceTheme
                }
            }
        }
    }

    StancePopup {
        id: popup
        service: stanceService
        theme: stanceTheme
        screen: {
            const name = Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : "";
            return Quickshell.screens.find(s => s.name === name) || Quickshell.screens[0];
        }
    }

    IpcHandler {
        target: "stance"

        function toggle(): void { popup.open = !popup.open; }
        function open(): void { popup.open = true; }
        function close(): void { popup.open = false; }
        function settings(): void { popup.settingsOpen = true; }
        function confirm(): void { stanceService.confirm(); }
        function revert(): void { stanceService.revert(); }
    }
}
