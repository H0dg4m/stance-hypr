import QtQuick

// Colours and metrics. Override any property to match your shell, e.g.
// StanceTheme { accent: Appearance.colors.colPrimary }
QtObject {
    property color background: "#1e1f24"
    // Section cards sit on the background; chips and controls on the cards.
    property color card: "#26282e"
    property color surface: "#2f323a"
    property color surfaceHover: "#3a3d47"
    property color border: "#3d404b"
    property color text: "#e6e7ec"
    property color textDim: "#9a9daa"
    property color accent: "#8ab4f8"
    property color accentText: "#10213a"
    // Soft accent for "on" tiles that should not shout.
    property color accentSoft: Qt.rgba(accent.r, accent.g, accent.b, 0.18)
    property color danger: "#f28b82"
    property color dangerText: "#3b0b08"
    property real radius: 12
    property real chipRadius: 8
    property string fontFamily: "sans-serif"
    property real fontSize: 13
    // A Material Symbols family ("Material Symbols Rounded") turns icons on;
    // empty keeps the components free of any icon font.
    property string iconFont: ""
    property int animMs: 120
    // Inner padding of section cards and gap between rows inside them.
    property real padding: 12
    property real spacing: 8
}
