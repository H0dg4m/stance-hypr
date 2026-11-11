import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland

// The panel in a layer-shell popup. With `anchorItem` (a bar button) it
// opens under that button; without, at the top-right of `screen`. Its
// settings chip opens StanceSettingsWindow in the middle of the same screen.
Scope {
    id: root

    required property StanceService service
    property StanceTheme theme: StanceTheme {}
    property Item anchorItem: null
    property var screen: null
    property bool open: false
    property bool bottom: false
    property real barHeight: 0
    property real edgeMargin: 8
    property alias settingsOpen: settingsWindow.open
    // The board's clipboard tile. Unhandled, it asks this shell over IPC for
    // the end4 shelf (`stanceclip`); the standalone shell opens its own.
    signal clipRequested
    property bool clipHandled: false
    onClipRequested: if (!clipHandled) Quickshell.execDetached(["qs", "-p", Quickshell.shellDir, "ipc", "call", "stanceclip", "open"])

    onOpenChanged: if (open) service.refresh()

    // Opened from the panel, which closes so the window is not hidden under it.
    StanceSettingsWindow {
        id: settingsWindow
        service: root.service
        theme: root.theme
        screen: root.anchorItem && root.anchorItem.QsWindow.window ? root.anchorItem.QsWindow.window.screen : root.screen
    }

    LazyLoader {
        active: root.open

        component: PanelWindow {
            id: win
            screen: root.anchorItem ? root.anchorItem.QsWindow.window.screen : root.screen
            color: "transparent"
            anchors.top: !root.bottom
            anchors.bottom: root.bottom
            anchors.left: root.anchorItem !== null
            anchors.right: root.anchorItem === null
            implicitWidth: frame.implicitWidth
            implicitHeight: frame.implicitHeight
            exclusionMode: ExclusionMode.Ignore
            exclusiveZone: 0
            WlrLayershell.namespace: "stance:panel"
            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

            margins {
                top: root.barHeight + root.edgeMargin
                bottom: root.barHeight + root.edgeMargin
                right: root.edgeMargin
                left: {
                    if (!root.anchorItem)
                        return 0;
                    const p = root.anchorItem.QsWindow.window.contentItem.mapFromItem(root.anchorItem, root.anchorItem.width / 2, 0);
                    const sw = win.screen ? win.screen.width : 1920;
                    return Math.max(root.edgeMargin, Math.min(p.x - frame.implicitWidth / 2, sw - frame.implicitWidth - root.edgeMargin));
                }
            }

            // Click outside closes the panel.
            HyprlandFocusGrab {
                active: true
                windows: [win]
                onCleared: root.open = false
            }

            Rectangle {
                id: frame
                // Taller than the screen (long mode editor): scroll instead of
                // running off the edge.
                readonly property real maxHeight: (win.screen ? win.screen.height : 1080) - root.barHeight - 3 * root.edgeMargin
                implicitWidth: content.width + 24
                implicitHeight: Math.min(maxHeight, content.implicitHeight + 24)
                radius: root.theme.radius
                color: root.theme.background
                border.width: 1
                border.color: root.theme.border
                focus: true
                Keys.onEscapePressed: root.open = false

                // Fade and slide a few px from the bar; height is not animated
                // because sections fill in as their data arrives.
                opacity: 0
                transform: Translate { id: slide; y: root.bottom ? 6 : -6 }
                Component.onCompleted: { fadeIn.start(); slideIn.start(); }
                NumberAnimation { id: fadeIn; target: frame; property: "opacity"; to: 1; duration: root.theme.animMs * 1.5; easing.type: Easing.OutCubic }
                NumberAnimation { id: slideIn; target: slide; property: "y"; to: 0; duration: root.theme.animMs * 1.5; easing.type: Easing.OutCubic }

                Flickable {
                    id: flick
                    x: 12
                    y: 12
                    width: content.width
                    height: parent.height - 24
                    clip: true
                    contentWidth: content.width
                    contentHeight: content.implicitHeight
                    // Only when it does not fit, so the screen map keeps its drag.
                    interactive: contentHeight > height
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar {
                        policy: flick.interactive ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
                    }

                    StancePanel {
                        id: content
                        service: root.service
                        theme: root.theme
                        preferredName: win.screen ? win.screen.name : ""
                        onSettingsRequested: {
                            root.settingsOpen = true;
                            root.open = false;
                        }
                        onClipRequested: {
                            root.open = false;
                            root.clipRequested();
                        }
                    }
                }
            }
        }
    }
}
