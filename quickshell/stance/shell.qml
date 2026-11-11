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
        // STANCE_BIN points the shell at another binary (tests use contrib/fake-clip.sh).
        bin: Quickshell.env("STANCE_BIN") || "stance"
    }

    StanceClipService {
        id: clipService
        bin: stanceService.bin
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

    StanceClip {
        id: clipShelf
        clip: clipService
        theme: stanceTheme
        screen: popup.screen
        onSettingsRequested: popup.settingsOpen = true
    }

    IpcHandler {
        target: "stance"

        function toggle(): void { popup.open = !popup.open; }
        function open(): void { popup.open = true; }
        function close(): void { popup.open = false; }
        function settings(): void { popup.settingsOpen = true; }
        // Clipboard shelf: `ipc call stance clip` toggles it.
        function clip(): void { clipShelf.open = !clipShelf.open; }
        function clipOpen(): void { clipShelf.open = true; }
        function clipClose(): void { clipShelf.open = false; }
        function confirm(): void { stanceService.confirm(); }
        function revert(): void { stanceService.revert(); }
    }
}
