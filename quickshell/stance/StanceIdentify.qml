import QtQuick
import Quickshell
import Quickshell.Wayland

// "Which screen is which": a big name card in the middle of every screen
// for a few seconds. Activate with `active = true`; it turns itself off.
Scope {
    id: root

    required property StanceTheme theme
    required property var monitors
    property bool active: false

    Timer {
        running: root.active
        interval: 2500
        onTriggered: root.active = false
    }

    Variants {
        model: root.active ? Quickshell.screens : []
        delegate: PanelWindow {
            id: win
            required property var modelData
            readonly property var mon: root.monitors.find(m => m.name === modelData.name) || null
            screen: modelData
            color: "transparent"
            implicitWidth: card.implicitWidth
            implicitHeight: card.implicitHeight
            exclusionMode: ExclusionMode.Ignore
            WlrLayershell.namespace: "stance:identify"
            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
            mask: Region {}

            Rectangle {
                id: card
                implicitWidth: col.implicitWidth + 64
                implicitHeight: col.implicitHeight + 40
                radius: root.theme.radius * 1.5
                color: root.theme.backgroundSolid
                border.width: 2
                border.color: root.theme.accent
                opacity: 0
                Component.onCompleted: opacity = 1
                Behavior on opacity { NumberAnimation { duration: root.theme.animMs * 2 } }
                Column {
                    id: col
                    anchors.centerIn: parent
                    spacing: 4
                    Text {
                        textFormat: Text.PlainText
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: win.modelData.name
                        color: root.theme.accent
                        font.family: root.theme.fontFamily
                        font.pixelSize: 56
                        font.weight: Font.Bold
                    }
                    Text {
                        textFormat: Text.PlainText
                        anchors.horizontalCenter: parent.horizontalCenter
                        visible: !!win.mon
                        text: win.mon ? (win.mon.description ? win.mon.description + "\n" : "") + win.mon.width + "×" + win.mon.height + " @ " + Math.round(win.mon.refresh) + " Hz" : ""
                        horizontalAlignment: Text.AlignHCenter
                        color: root.theme.textDim
                        font.family: root.theme.fontFamily
                        font.pixelSize: root.theme.fontSize + 2
                    }
                }
            }
        }
    }
}
