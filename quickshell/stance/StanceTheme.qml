import QtQuick

// Colours and metrics. Override any property to match your shell, e.g.
// StanceTheme { accent: Appearance.colors.colPrimary }
QtObject {
    property color background: "#1e1f24"
    property color surface: "#2a2c33"
    property color surfaceHover: "#353842"
    property color border: "#3d404b"
    property color text: "#e6e7ec"
    property color textDim: "#9a9daa"
    property color accent: "#8ab4f8"
    property color accentText: "#10213a"
    property color danger: "#f28b82"
    property color dangerText: "#3b0b08"
    property real radius: 12
    property real chipRadius: 8
    property string fontFamily: "sans-serif"
    property real fontSize: 13
}
