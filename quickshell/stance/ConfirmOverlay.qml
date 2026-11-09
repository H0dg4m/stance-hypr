import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland

// "Keep these display settings?" card. Create one per screen: whichever
// screen still shows a picture can confirm. Doing nothing reverts.
LazyLoader {
    id: root

    required property StanceService service
    required property var screen
    property StanceTheme theme: StanceTheme {}
    // Set false for hidden duplicates (e.g. a bar that keeps an invisible
    // alternate layout) so each screen gets exactly one card.
    property bool enabled: true

    active: enabled && service.pending !== null

    component: PanelWindow {
        id: win
        screen: root.screen
        color: "transparent"
        anchors.top: true
        margins.top: 72
        implicitWidth: card.implicitWidth
        implicitHeight: card.implicitHeight
        exclusionMode: ExclusionMode.Ignore
        exclusiveZone: 0
        WlrLayershell.namespace: "stance:confirm"
        WlrLayershell.layer: WlrLayer.Overlay
        readonly property bool focusedScreen: Hyprland.focusedMonitor && root.screen && Hyprland.focusedMonitor.name === root.screen.name
        WlrLayershell.keyboardFocus: focusedScreen ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        // Ignore keys for a moment so an Enter that triggered the change
        // cannot also confirm it.
        property bool armed: false
        Timer {
            interval: 600
            running: true
            onTriggered: win.armed = true
        }

        Rectangle {
            id: card
            implicitWidth: 360
            implicitHeight: col.implicitHeight + 32
            radius: root.theme.radius
            color: root.theme.background
            border.width: 1
            border.color: root.theme.border
            focus: true

            Keys.onPressed: event => {
                if (!win.armed)
                    return;
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
                    root.service.confirm();
                    event.accepted = true;
                } else if (event.key === Qt.Key_Escape) {
                    root.service.revert();
                    event.accepted = true;
                }
            }

            ColumnLayout {
                id: col
                anchors.fill: parent
                anchors.margins: 16
                spacing: 10

                Text {
                    text: "Keep these display settings?"
                    color: root.theme.text
                    font.family: root.theme.fontFamily
                    font.pixelSize: root.theme.fontSize + 3
                    font.weight: Font.DemiBold
                }
                Text {
                    text: (root.service.pending ? root.service.pending.label : "") + " · reverting in " + Math.ceil(root.service.remainingMs / 1000) + " s"
                    color: root.theme.textDim
                    font.family: root.theme.fontFamily
                    font.pixelSize: root.theme.fontSize
                }
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 4
                    radius: 2
                    color: root.theme.surface
                    Rectangle {
                        width: parent.width * Math.min(1, root.service.remainingMs / root.service.timeoutMs)
                        height: parent.height
                        radius: 2
                        color: root.theme.accent
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    Item { Layout.fillWidth: true }
                    StanceChip {
                        theme: root.theme
                        label: "Revert (Esc)"
                        onClicked: root.service.revert()
                    }
                    StanceChip {
                        theme: root.theme
                        label: "Keep (Enter)"
                        selected: true
                        onClicked: root.service.confirm()
                    }
                }
            }
        }
    }
}
