import QtQuick

// Small clickable pill used for actions in the panel, with an optional icon.
Rectangle {
    id: chip

    required property StanceTheme theme
    property string label: ""
    property string icon: ""
    property bool selected: false
    property bool danger: false
    property string tip: ""
    // Long labels (audio device names) elide instead of overflowing.
    property real maxWidth: 100000
    signal clicked

    readonly property bool hasIcon: icon !== "" && theme.iconFont !== ""
    readonly property color fg: selected ? (danger ? theme.dangerText : theme.accentText) : (danger ? theme.danger : theme.text)

    implicitWidth: Math.min(content.implicitWidth + 20, maxWidth)
    implicitHeight: 30
    radius: theme.chipRadius
    opacity: enabled ? 1 : 0.45
    color: selected ? (danger ? theme.danger : theme.accent)
                    : (mouse.containsMouse && enabled ? theme.surfaceHover : theme.surface)
    activeFocusOnTab: enabled
    Accessible.role: Accessible.Button
    Accessible.name: label !== "" ? label : tip
    Accessible.description: tip
    Accessible.onPressAction: if (enabled) clicked()
    Keys.onSpacePressed: if (enabled) clicked()
    Keys.onReturnPressed: if (enabled) clicked()
    Keys.onEnterPressed: if (enabled) clicked()
    border.width: activeFocus ? 2 : 0
    border.color: theme.accent
    scale: mouse.pressed && enabled ? 0.97 : 1

    Behavior on color { ColorAnimation { duration: chip.theme.animMs } }
    Behavior on scale { NumberAnimation { duration: 80 } }

    Row {
        id: content
        anchors.centerIn: parent
        spacing: 6
        StanceIcon {
            theme: chip.theme
            name: chip.icon
            size: chip.theme.fontSize + 3
            color: chip.fg
            fill: chip.selected
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            id: labelText
            textFormat: Text.PlainText
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, chip.maxWidth - 20 - (chip.hasIcon ? chip.theme.fontSize + 9 : 0))
            elide: Text.ElideRight
            text: chip.label
            visible: text !== ""
            color: chip.fg
            font.family: chip.theme.fontFamily
            font.pixelSize: chip.theme.fontSize - 1
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: chip.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: if (chip.enabled) chip.clicked()
    }
}
