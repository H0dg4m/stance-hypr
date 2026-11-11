import QtQuick
import QtQuick.Layouts

// Category navigation. `vertical`: a sidebar list (icon + label). Otherwise a
// horizontal segmented row that scrolls sideways; there the picked entry shows
// its label and the others only their icon (text only when no icon font).
// model: [{ id, label, icon }]; `current` is an id.
Item {
    id: nav

    required property StanceTheme theme
    property var model: []
    property string current: ""
    property bool vertical: true
    signal picked(string id)

    readonly property bool icons: theme.iconFont !== ""

    implicitWidth: vertical ? 168 : 200
    implicitHeight: vertical ? col.implicitHeight : 38

    // Sidebar
    Column {
        id: col
        visible: nav.vertical
        width: parent.width
        spacing: 2
        Repeater {
            model: nav.vertical ? nav.model : []
            delegate: Rectangle {
                id: item
                required property var modelData
                readonly property bool on: modelData.id === nav.current
                width: col.width
                height: 36
                radius: nav.theme.chipRadius
                color: on ? nav.theme.accent : (ma.containsMouse ? nav.theme.surface : "transparent")
                Behavior on color { ColorAnimation { duration: nav.theme.animMs } }
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    x: 10
                    spacing: 9
                    StanceIcon {
                        visible: nav.icons
                        theme: nav.theme
                        name: item.modelData.icon
                        size: nav.theme.fontSize + 4
                        fill: item.on
                        color: item.on ? nav.theme.accentText : nav.theme.textDim
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        width: item.width - 20 - (nav.icons ? nav.theme.fontSize + 13 : 0)
                        elide: Text.ElideRight
                        text: item.modelData.label
                        color: item.on ? nav.theme.accentText : nav.theme.text
                        font.family: nav.theme.fontFamily
                        font.pixelSize: nav.theme.fontSize
                        font.weight: item.on ? Font.DemiBold : Font.Normal
                    }
                }
                MouseArea {
                    id: ma
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: nav.picked(item.modelData.id)
                }
            }
        }
    }

    // Top row
    Rectangle {
        visible: !nav.vertical
        anchors.fill: parent
        radius: nav.theme.chipRadius + 3
        color: nav.theme.background
        Flickable {
            id: flick
            anchors.fill: parent
            anchors.margins: 3
            clip: true
            contentWidth: row.implicitWidth
            contentHeight: height
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.HorizontalFlick
            // Mouse wheel scrolls sideways.
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse
                onWheel: e => flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width, flick.contentX - (e.angleDelta.y + e.angleDelta.x) / 2))
            }
            Row {
                id: row
                height: flick.height
                spacing: 2
                Repeater {
                    model: nav.vertical ? [] : nav.model
                    delegate: Rectangle {
                        id: cell
                        required property var modelData
                        readonly property bool on: modelData.id === nav.current
                        readonly property bool showText: !nav.icons || on
                        height: row.height
                        width: cellRow.implicitWidth + 18
                        radius: nav.theme.chipRadius
                        color: on ? nav.theme.accent : (cma.containsMouse ? nav.theme.surface : "transparent")
                        Behavior on color { ColorAnimation { duration: nav.theme.animMs } }
                        onOnChanged: if (on) nav.reveal(cell)
                        Row {
                            id: cellRow
                            anchors.centerIn: parent
                            spacing: 6
                            StanceIcon {
                                visible: nav.icons
                                theme: nav.theme
                                name: cell.modelData.icon
                                size: nav.theme.fontSize + 3
                                fill: cell.on
                                color: cell.on ? nav.theme.accentText : nav.theme.textDim
                                anchors.verticalCenter: parent.verticalCenter
                            }
                            Text {
                                visible: cell.showText
                                anchors.verticalCenter: parent.verticalCenter
                                text: cell.modelData.label
                                color: cell.on ? nav.theme.accentText : nav.theme.text
                                font.family: nav.theme.fontFamily
                                font.pixelSize: nav.theme.fontSize - 1
                                font.weight: cell.on ? Font.DemiBold : Font.Normal
                            }
                        }
                        MouseArea {
                            id: cma
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: nav.picked(cell.modelData.id)
                        }
                    }
                }
            }
        }
    }

    // Scroll the picked cell into view.
    function reveal(cell) {
        Qt.callLater(() => {
            if (!cell || !flick)
                return;
            if (cell.x < flick.contentX)
                flick.contentX = Math.max(0, cell.x - 6);
            else if (cell.x + cell.width > flick.contentX + flick.width)
                flick.contentX = Math.min(flick.contentWidth - flick.width, cell.x + cell.width - flick.width + 6);
        });
    }
}
