import QtQuick

// Single-line text field. The layer popup takes keyboard focus on demand,
// so a click on the field focuses it; every action also has a button.
Rectangle {
    id: field
    required property StanceTheme theme
    property alias text: input.text
    property string placeholder: ""
    property bool invalid: false
    property int maxLength: 200
    property var validator: null
    signal accepted
    signal cancelled
    signal edited(string value)

    function focusInput() { input.forceActiveFocus(); }

    implicitHeight: 34
    radius: theme.chipRadius
    color: theme.surface
    border.width: 2
    border.color: invalid ? theme.danger : (input.activeFocus ? theme.accent : "transparent")
    opacity: enabled ? 1 : 0.45
    Behavior on border.color { ColorAnimation { duration: field.theme.animMs } }

    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.IBeamCursor
        onClicked: input.forceActiveFocus()
    }
    TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        verticalAlignment: TextInput.AlignVCenter
        color: field.theme.text
        selectionColor: field.theme.accent
        selectedTextColor: field.theme.accentText
        font.family: field.theme.fontFamily
        font.pixelSize: field.theme.fontSize
        maximumLength: field.maxLength
        validator: field.validator
        selectByMouse: true
        clip: true
        onTextEdited: field.edited(text)
        onAccepted: field.accepted()
        Keys.onEscapePressed: field.cancelled()
    }
    Text {
        anchors.fill: input
        verticalAlignment: Text.AlignVCenter
        visible: input.text === "" && !input.preeditText
        text: field.placeholder
        color: field.theme.textDim
        font.family: field.theme.fontFamily
        font.pixelSize: field.theme.fontSize
        elide: Text.ElideRight
    }
}
