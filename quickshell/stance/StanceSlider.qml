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
    signal committed(real value)

    onValueChanged: awaiting = false
    onCommitted: { awaiting = true; holdTimer.restart(); }

    implicitHeight: 20
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
        height: 6
        radius: 3
        color: slider.theme.surface
        border.width: 1
        border.color: slider.theme.border
        Rectangle {
            width: Math.max(6, track.width * (slider.shown - slider.from) / (slider.to - slider.from))
            height: parent.height
            radius: 3
            color: slider.theme.accent
        }
    }
    Rectangle {
        width: 14
        height: 14
        radius: 7
        anchors.verticalCenter: parent.verticalCenter
        x: Math.max(0, Math.min(slider.width - width, track.width * (slider.shown - slider.from) / (slider.to - slider.from) - width / 2))
        color: slider.theme.accent
        border.width: 2
        border.color: slider.theme.background
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        cursorShape: slider.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onPressed: e => slider.dragValue = slider.valueAt(e.x)
        onPositionChanged: e => { if (pressed) slider.dragValue = slider.valueAt(e.x); }
        onReleased: e => {
            slider.dragValue = slider.valueAt(e.x);
            if (slider.dragValue !== slider.value)
                slider.committed(slider.dragValue);
        }
        // Wheel ticks accumulate and commit once the wheel rests.
        onWheel: w => {
            if (w.angleDelta.y === 0)
                return;
            const base = (wheelTimer.running || slider.awaiting) ? slider.dragValue : slider.value;
            slider.dragValue = Math.max(slider.from, Math.min(slider.to, base + (w.angleDelta.y > 0 ? 5 : -5)));
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
