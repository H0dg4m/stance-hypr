import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland

// The panel in a layer-shell popup. With `anchorItem` (a bar button) it
// opens under that button; without, at the top-right of `screen`.
LazyLoader {
    id: root

    required property StanceService service
    property StanceTheme theme: StanceTheme {}
    property Item anchorItem: null
    property var screen: null
    property bool open: false
    property bool bottom: false
    property real barHeight: 0
    property real edgeMargin: 8

    active: open
    onOpenChanged: if (open) service.refresh()

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
            implicitWidth: content.width + 24
            implicitHeight: content.implicitHeight + 24
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

            StancePanel {
                id: content
                x: 12
                y: 12
                service: root.service
                theme: root.theme
                preferredName: win.screen ? win.screen.name : ""
            }
        }
    }
}
