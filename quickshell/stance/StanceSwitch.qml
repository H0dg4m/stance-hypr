import QtQuick

// Small on/off switch.
Rectangle {
    id: sw
    required property StanceTheme theme
    property bool checked: false
    // Spoken name for screen readers (the label next to the switch).
    property string tip: ""
    signal toggled(bool value)

    implicitWidth: 40
    implicitHeight: 24
    radius: 12
    opacity: enabled ? 1 : 0.45
    color: checked ? theme.accent : theme.surface
    activeFocusOnTab: enabled
    Accessible.role: Accessible.CheckBox
    Accessible.name: tip
    Accessible.checked: checked
    Accessible.onToggleAction: if (enabled) toggled(!checked)
    Keys.onSpacePressed: if (enabled) toggled(!checked)
    Keys.onReturnPressed: if (enabled) toggled(!checked)
    Keys.onEnterPressed: if (enabled) toggled(!checked)
    border.width: checked && !activeFocus ? 0 : 2
    border.color: activeFocus ? theme.accent : theme.textDim
    Behavior on color { ColorAnimation { duration: sw.theme.animMs } }

    Rectangle {
        width: sw.checked ? 16 : 12
        height: width
        radius: width / 2
        anchors.verticalCenter: parent.verticalCenter
        x: sw.checked ? parent.width - width - 4 : 6
        color: sw.checked ? sw.theme.accentText : sw.theme.textDim
        Behavior on x { NumberAnimation { duration: sw.theme.animMs; easing.type: Easing.OutCubic } }
        Behavior on width { NumberAnimation { duration: sw.theme.animMs } }
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: sw.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: if (sw.enabled) sw.toggled(!sw.checked)
    }
}
