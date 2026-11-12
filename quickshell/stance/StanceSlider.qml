import QtQuick

// Minimal 0-100 slider without QtQuick.Controls. Shows the drag live but
// emits `committed` only on release: DDC/CI writes take ~0.3 s each.
Item {
    id: slider

    required property StanceTheme theme
    property real value: 0
    property real from: 0
    property real to: 100
    property real dragValue: value
    readonly property bool dragging: mouse.pressed || wheelTimer.running
    // After a commit, keep showing the new value until the re-read arrives.
    property bool awaiting: false
    readonly property real shown: dragging || awaiting ? dragValue : value
    readonly property real handleSize: mouse.pressed || mouse.containsMouse ? 20 : 16
    signal committed(real value)
    // While dragging or wheeling (live preview); `committed` still fires once on release.
    signal moved(real value)

    onValueChanged: awaiting = false
    onCommitted: { awaiting = true; holdTimer.restart(); }

    property string label: ""
    activeFocusOnTab: enabled
    Accessible.role: Accessible.Slider
    Accessible.name: label
    Accessible.focusable: true
    Keys.onLeftPressed: step(-1)
    Keys.onDownPressed: step(-1)
    Keys.onRightPressed: step(1)
    Keys.onUpPressed: step(1)
    function step(d) {
        const v = Math.max(from, Math.min(to, Math.round(shown) + d));
        if (v === Math.round(shown))
            return;
        dragValue = v;
        moved(v);
        // Held arrow keys auto-repeat: commit once they rest (one hardware write).
        wheelTimer.restart();
    }

    implicitHeight: 24
    implicitWidth: 160
    opacity: enabled ? 1 : 0.45

    function valueAt(x) {
        const f = Math.max(0, Math.min(1, x / width));
        return Math.round(from + f * (to - from));
    }

    Rectangle {
        id: track
        anchors.verticalCenter: parent.verticalCenter
        width: parent.width
        height: 8
        radius: 4
        color: slider.theme.surface
        Rectangle {
            width: Math.max(8, track.width * (slider.shown - slider.from) / (slider.to - slider.from))
            height: parent.height
            radius: 4
            color: slider.theme.accent
        }
    }
    Rectangle {
        width: handleSize
        height: handleSize
        radius: handleSize / 2
        anchors.verticalCenter: parent.verticalCenter
        x: Math.max(0, Math.min(slider.width - width, track.width * (slider.shown - slider.from) / (slider.to - slider.from) - width / 2))
        color: slider.theme.accent
        border.width: 3
        border.color: slider.activeFocus ? slider.theme.text : slider.theme.card
        Behavior on width { NumberAnimation { duration: 80 } }
        Behavior on height { NumberAnimation { duration: 80 } }
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: slider.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onPressed: e => { slider.dragValue = slider.valueAt(e.x); slider.moved(slider.dragValue); }
        onPositionChanged: e => { if (pressed) { slider.dragValue = slider.valueAt(e.x); slider.moved(slider.dragValue); } }
        onReleased: e => {
            slider.dragValue = slider.valueAt(e.x);
            if (slider.dragValue !== slider.value)
                slider.committed(slider.dragValue);
        }
        // Wheel ticks (and arrow keys) accumulate and commit once they rest.
        onWheel: w => {
            if (w.angleDelta.y === 0)
                return;
            const base = (wheelTimer.running || slider.awaiting) ? slider.dragValue : slider.value;
            slider.dragValue = Math.max(slider.from, Math.min(slider.to, base + (w.angleDelta.y > 0 ? 5 : -5)));
            slider.moved(slider.dragValue);
            wheelTimer.restart();
        }
    }
    Timer {
        id: holdTimer
        interval: 4000
        onTriggered: slider.awaiting = false
    }
    Timer {
        id: wheelTimer
        interval: 350
        onTriggered: if (slider.dragValue !== slider.value) slider.committed(slider.dragValue)
    }
}
