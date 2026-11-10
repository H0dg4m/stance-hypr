import QtQuick
import QtQuick.Layouts

// A rounded card with an icon + title header. Children go into a column;
// `trailing` items sit on the right of the header.
Rectangle {
    id: section

    required property StanceTheme theme
    property string icon: ""
    property string title: ""
    default property alias content: body.data
    property alias trailing: trail.data
    property real padding: theme.padding

    implicitHeight: col.implicitHeight + padding * 2
    radius: theme.radius
    color: theme.card

    ColumnLayout {
        id: col
        x: section.padding
        y: section.padding
        width: section.width - section.padding * 2
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
            visible: section.title !== ""
            spacing: 8
            StanceIcon { theme: section.theme; name: section.icon; size: section.theme.fontSize + 3; color: section.theme.textDim }
            Text {
                text: section.title
                color: section.theme.textDim
                font.family: section.theme.fontFamily
                font.pixelSize: section.theme.fontSize - 1
                font.weight: Font.Medium
                Layout.fillWidth: true
            }
            RowLayout { id: trail; spacing: 6 }
        }

        ColumnLayout {
            id: body
            Layout.fillWidth: true
            spacing: section.theme.spacing
        }
    }
}
