import QtQuick
import QtQuick.Layouts

// Segmented button group: one inset track, the picked segment in accent.
// model: [{ label, value }]; `current` is compared with each value.
Rectangle {
    id: seg

    required property StanceTheme theme
    property var model: []
    property var current: null
    // Optional comparison for values like refresh rates (59.95 vs 60).
    property var same: (a, b) => a === b
    signal picked(var value)

    implicitHeight: 32
    implicitWidth: row.implicitWidth + 6
    radius: theme.chipRadius + 3
    color: theme.background
    opacity: enabled ? 1 : 0.45
    activeFocusOnTab: enabled
    Accessible.role: Accessible.Grouping
    Accessible.name: model.map(m => m.label).join(", ")
    border.width: activeFocus ? 2 : 0
    border.color: theme.accent
    // Arrow keys step through the segments.
    function step(d) {
        const i = model.findIndex(m => current !== null && same(m.value, current));
        const j = Math.max(0, Math.min(model.length - 1, (i < 0 ? 0 : i + d)));
        if (model.length > 0 && j !== i)
            picked(model[j].value);
    }
    Keys.onLeftPressed: if (enabled) step(-1)
    Keys.onRightPressed: if (enabled) step(1)

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.margins: 3
        spacing: 2
        Repeater {
            model: seg.model
            delegate: Rectangle {
                id: cell
                required property var modelData
                readonly property bool on: seg.current !== null && seg.same(modelData.value, seg.current)
                Layout.fillWidth: true
                Layout.fillHeight: true
                implicitWidth: lbl.implicitWidth + 18
                radius: seg.theme.chipRadius
                color: on ? seg.theme.accent : (ma.containsMouse && seg.enabled ? seg.theme.surface : "transparent")
                Behavior on color { ColorAnimation { duration: seg.theme.animMs } }
                Text {
                    textFormat: Text.PlainText
                    id: lbl
                    anchors.centerIn: parent
                    text: cell.modelData.label
                    color: cell.on ? seg.theme.accentText : seg.theme.text
                    font.family: seg.theme.fontFamily
                    font.pixelSize: seg.theme.fontSize - 1
                    font.weight: cell.on ? Font.DemiBold : Font.Normal
                }
                MouseArea {
                    id: ma
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: seg.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onClicked: if (seg.enabled && !cell.on) seg.picked(cell.modelData.value)
                }
            }
        }
    }
}
