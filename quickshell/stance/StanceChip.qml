import QtQuick

// Small clickable pill used for every action in the panel.
Rectangle {
    id: chip

    required property StanceTheme theme
    property string label: ""
    property bool selected: false
    property bool danger: false
    property string tip: ""
    // Long labels (audio device names) elide instead of overflowing.
    property real maxWidth: 100000
    signal clicked

    implicitWidth: Math.min(labelText.implicitWidth + 20, maxWidth)
    implicitHeight: 28
    radius: theme.chipRadius
    opacity: enabled ? 1 : 0.45
    color: selected ? (danger ? theme.danger : theme.accent)
                    : (mouse.containsMouse && enabled ? theme.surfaceHover : theme.surface)
    border.width: selected ? 0 : 1
    border.color: theme.border

    Behavior on color { ColorAnimation { duration: 90 } }

    Text {
        id: labelText
        anchors.centerIn: parent
        width: Math.min(implicitWidth, chip.maxWidth - 20)
        elide: Text.ElideRight
        text: chip.label
        color: chip.selected ? (chip.danger ? chip.theme.dangerText : chip.theme.accentText) : chip.theme.text
        font.family: chip.theme.fontFamily
        font.pixelSize: chip.theme.fontSize
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: chip.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: if (chip.enabled) chip.clicked()
    }
}
