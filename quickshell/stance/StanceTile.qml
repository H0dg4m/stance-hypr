import QtQuick
import QtQuick.Layouts

// Half-width board tile (the S size of a widget): icon, title, one value
// line. Click = the widget's main action; the wheel is optional (`wheel`
// gets +1 / -1). `on` tints it like a selected chip.
Rectangle {
    id: tile

    required property StanceTheme theme
    property string icon: ""
    property string title: ""
    property string value: ""
    property bool on: false
    property bool busy: false
    property string tip: ""
    signal clicked
    signal wheel(int step)

    implicitHeight: 64
    radius: theme.radius
    color: on ? theme.accentSoft : (mouse.containsMouse && enabled ? theme.surfaceHover : theme.card)
    opacity: enabled ? (busy ? 0.6 : 1) : 0.45
    scale: mouse.pressed && enabled ? 0.98 : 1
    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: title !== "" ? title : tip
    Accessible.description: value
    Accessible.onPressAction: clicked()
    border.width: activeFocus ? 2 : 0
    border.color: theme.accent

    Behavior on color { ColorAnimation { duration: tile.theme.animMs } }
    Behavior on scale { NumberAnimation { duration: 80 } }

    Keys.onPressed: e => {
        if (e.key === Qt.Key_Space || e.key === Qt.Key_Return || e.key === Qt.Key_Enter) {
            tile.clicked();
            e.accepted = true;
        } else if (e.key === Qt.Key_Up || e.key === Qt.Key_Right) {
            tile.wheel(1);
            e.accepted = true;
        } else if (e.key === Qt.Key_Down || e.key === Qt.Key_Left) {
            tile.wheel(-1);
            e.accepted = true;
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 10
        spacing: 10
        StanceIcon {
            theme: tile.theme
            name: tile.icon
            size: tile.theme.fontSize + 7
            fill: tile.on
            color: tile.on ? tile.theme.accent : tile.theme.textDim
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 1
            Text {
                textFormat: Text.PlainText
                text: tile.title
                color: tile.theme.text
                font.family: tile.theme.fontFamily
                font.pixelSize: tile.theme.fontSize - 1
                font.weight: Font.Medium
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Text {
                textFormat: Text.PlainText
                visible: text !== ""
                text: tile.value
                color: tile.on ? tile.theme.accent : tile.theme.textDim
                font.family: tile.theme.fontFamily
                font.pixelSize: tile.theme.fontSize - 2
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: tile.clicked()
        onWheel: w => {
            if (w.angleDelta.y !== 0)
                tile.wheel(w.angleDelta.y > 0 ? 1 : -1);
        }
    }
}
