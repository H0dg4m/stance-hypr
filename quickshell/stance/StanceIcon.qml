import QtQuick

// Material Symbols glyph by name; renders nothing without theme.iconFont.
Text {
    id: icon
    required property StanceTheme theme
    property string name: ""
    property real size: theme.fontSize + 5
    property bool fill: false

    visible: theme.iconFont !== "" && name !== ""
    text: name
    color: theme.text
    font.family: theme.iconFont
    font.pixelSize: size
    font.variableAxes: ({ "FILL": fill ? 1 : 0, "opsz": 24 })
    verticalAlignment: Text.AlignVCenter
    horizontalAlignment: Text.AlignHCenter
}
