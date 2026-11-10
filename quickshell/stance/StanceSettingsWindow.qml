import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

// Panel settings in their own window, centred on `screen`. The page is
// longer than most screens, so it scrolls; Esc or the close chip shut it.
LazyLoader {
    id: root

    required property StanceService service
    property StanceTheme theme: StanceTheme {}
    property var screen: null
    property bool open: false

    active: open

    component: PanelWindow {
        id: win
        screen: root.screen
        color: "transparent"
        // No anchors: the compositor centres the surface.
        implicitWidth: frame.implicitWidth
        implicitHeight: frame.implicitHeight
        exclusionMode: ExclusionMode.Ignore
        exclusiveZone: 0
        WlrLayershell.namespace: "stance:settings"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

        readonly property real maxHeight: (win.screen ? win.screen.height : 1080) * 0.85

        Rectangle {
            id: frame
            implicitWidth: 460
            implicitHeight: Math.min(win.maxHeight, header.implicitHeight + settings.implicitHeight + 40)
            radius: root.theme.radius
            color: root.theme.background
            border.width: 1
            border.color: root.theme.border
            focus: true
            Keys.onEscapePressed: root.open = false

            opacity: 0
            Component.onCompleted: fadeIn.start()
            NumberAnimation { id: fadeIn; target: frame; property: "opacity"; to: 1; duration: root.theme.animMs * 1.5; easing.type: Easing.OutCubic }

            RowLayout {
                id: header
                x: 16
                y: 12
                width: parent.width - 32
                spacing: 8
                StanceIcon { visible: root.theme.iconFont !== ""; theme: root.theme; name: "settings"; color: root.theme.accent; fill: true }
                Text {
                    text: "Panel settings"
                    color: root.theme.text
                    font.family: root.theme.fontFamily
                    font.pixelSize: root.theme.fontSize + 3
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                }
                StanceChip {
                    theme: root.theme
                    icon: "close"
                    label: root.theme.iconFont === "" ? "Close" : ""
                    onClicked: root.open = false
                }
            }

            Flickable {
                id: flick
                x: 16
                y: header.y + header.implicitHeight + 12
                width: parent.width - 32
                height: parent.height - y - 12
                clip: true
                contentWidth: width
                contentHeight: settings.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {
                    policy: flick.contentHeight > flick.height ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
                }

                StanceSettings {
                    id: settings
                    // Leave room for the scroll bar.
                    width: flick.width - 12
                    service: root.service
                    theme: root.theme
                    onBack: root.open = false
                }
            }
        }
    }
}
