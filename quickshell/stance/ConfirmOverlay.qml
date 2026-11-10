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
            readonly property int secs: Math.ceil(root.service.remainingMs / 1000)
            readonly property bool urgent: secs <= 3
            implicitWidth: 380
            implicitHeight: col.implicitHeight + 40
            radius: root.theme.radius
            color: root.theme.background
            border.width: 1
            border.color: root.theme.border
            focus: true
            opacity: 0
            scale: 0.96
            Component.onCompleted: { opacity = 1; scale = 1; }
            Behavior on opacity { NumberAnimation { duration: root.theme.animMs; easing.type: Easing.OutCubic } }
            Behavior on scale { NumberAnimation { duration: root.theme.animMs; easing.type: Easing.OutCubic } }

            // Enter keeps, Escape reverts. Space is deliberately unbound.
            Keys.onPressed: event => {
                if (!win.armed)
                    return;
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
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
                anchors.margins: 20
                spacing: 14

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 12
                    Rectangle {
                        implicitWidth: 44
                        implicitHeight: 44
                        radius: 22
                        color: root.theme.accentSoft
                        StanceIcon {
                            anchors.centerIn: parent
                            theme: root.theme
                            name: "monitor"
                            size: 24
                            color: root.theme.accent
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Text {
                            text: "Keep these display settings?"
                            color: root.theme.text
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize + 3
                            font.weight: Font.DemiBold
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }
                        Text {
                            text: root.service.pending ? root.service.pending.label : ""
                            color: root.theme.textDim
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize - 1
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }
                    }
                    Text {
                        text: card.secs + " s"
                        color: card.urgent ? root.theme.danger : root.theme.accent
                        font.family: root.theme.fontFamily
                        font.pixelSize: root.theme.fontSize + 7
                        font.weight: Font.DemiBold
                        Behavior on color { ColorAnimation { duration: root.theme.animMs } }
                    }
                }
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 6
                    radius: 3
                    color: root.theme.surface
                    Rectangle {
                        width: parent.width * Math.min(1, root.service.remainingMs / root.service.timeoutMs)
                        height: parent.height
                        radius: 3
                        color: card.urgent ? root.theme.danger : root.theme.accent
                    }
                }
                Text {
                    text: "Reverting automatically in " + card.secs + " s if you do nothing."
                    color: root.theme.textDim
                    font.family: root.theme.fontFamily
                    font.pixelSize: root.theme.fontSize - 1
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    StanceChip {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 42
                        theme: root.theme
                        icon: "undo"
                        label: "Revert · Esc"
                        onClicked: root.service.revert()
                    }
                    StanceChip {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 42
                        theme: root.theme
                        icon: "check"
                        label: "Keep · Enter"
                        selected: true
                        onClicked: root.service.confirm()
                    }
                }
            }
        }
    }
}
