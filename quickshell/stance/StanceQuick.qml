import QtQuick
import QtQuick.Layouts

// Night light, brightness and audio output. Read with one `stance quick`
// call when the panel opens; each part hides itself when unavailable.
ColumnLayout {
    id: quick

    required property StanceService service
    required property StanceTheme theme
    readonly property var q: service.quick
    readonly property var night: q ? q.night : null
    readonly property var displays: q ? q.brightness : []
    readonly property var sinks: q ? q.sinks.filter(s => !s.virtual || s.default) : []

    spacing: 6
    visible: q !== null && (!!(night && night.available) || displays.length > 0 || sinks.length > 0)

    Rectangle {
        Layout.fillWidth: true
        Layout.topMargin: 4
        implicitHeight: 1
        color: quick.theme.border
    }

    // ---- night light ---------------------------------------------------
    RowLayout {
        visible: !!(quick.night && quick.night.available)
        Layout.fillWidth: true
        spacing: 6
        StanceLabel { theme: quick.theme; text: "Night light"; Layout.fillWidth: true }
        Repeater {
            model: [3500, 4500, 5500]
            delegate: StanceChip {
                required property int modelData
                theme: quick.theme
                label: (modelData / 1000).toFixed(1) + "k"
                selected: !!(quick.night && quick.night.on && quick.night.temperature === modelData)
                enabled: !quick.service.quickBusy
                onClicked: quick.service.runQuick(["night", "set", String(modelData)])
            }
        }
        StanceChip {
            theme: quick.theme
            label: quick.night && quick.night.on ? "On" : "Off"
            selected: !!(quick.night && quick.night.on)
            enabled: !quick.service.quickBusy
            onClicked: quick.service.runQuick(["night", "toggle"])
        }
    }

    // ---- brightness ----------------------------------------------------
    StanceLabel { theme: quick.theme; visible: quick.displays.length > 0; text: "Brightness" }
    Repeater {
        model: quick.displays
        delegate: RowLayout {
            required property var modelData
            Layout.fillWidth: true
            spacing: 8
            Text {
                text: modelData.monitor
                color: quick.theme.text
                font.family: quick.theme.fontFamily
                font.pixelSize: quick.theme.fontSize - 1
                Layout.preferredWidth: 52
                elide: Text.ElideRight
            }
            StanceSlider {
                id: sl
                theme: quick.theme
                Layout.fillWidth: true
                value: modelData.percent
                enabled: !quick.service.quickBusy
                onCommitted: v => quick.service.runQuick(["brightness", modelData.monitor, String(v)])
            }
            Text {
                text: Math.round(sl.shown) + "%"
                color: quick.theme.textDim
                font.family: quick.theme.fontFamily
                font.pixelSize: quick.theme.fontSize - 1
                horizontalAlignment: Text.AlignRight
                Layout.preferredWidth: 36
            }
        }
    }

    // ---- audio output ----------------------------------------------------
    StanceLabel { theme: quick.theme; visible: quick.sinks.length > 1; text: "Audio output" }
    Flow {
        visible: quick.sinks.length > 1
        Layout.fillWidth: true
        spacing: 6
        Repeater {
            model: quick.sinks
            delegate: StanceChip {
                required property var modelData
                theme: quick.theme
                label: modelData.description
                maxWidth: 336
                selected: modelData.default
                enabled: !quick.service.quickBusy
                onClicked: if (!selected) quick.service.runQuick(["audio", "set", String(modelData.id)])
            }
        }
    }

    Text {
        visible: text !== ""
        text: quick.service.quickError
        color: quick.theme.danger
        font.family: quick.theme.fontFamily
        font.pixelSize: quick.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
