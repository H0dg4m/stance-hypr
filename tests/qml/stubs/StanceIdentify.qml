import QtQuick
import Quickshell

// Smoke-test stand-in: the real one opens layer-shell windows, which the
// offscreen platform cannot create.
Scope {
    required property StanceTheme theme
    required property var monitors
    property bool active: false
}
