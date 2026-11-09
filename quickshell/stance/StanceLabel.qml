import QtQuick

// Secondary text in the theme's dim colour.
Text {
    required property StanceTheme theme
    color: theme.textDim
    font.family: theme.fontFamily
    font.pixelSize: theme.fontSize - 1
}
