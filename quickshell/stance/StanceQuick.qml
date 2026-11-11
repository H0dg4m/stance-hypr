import QtQuick
import QtQuick.Layouts

// Night light, brightness or audio output as a card (the M size of those
// widgets; `part` picks one, "" shows all three). Read with a single
// `stance quick` call when the panel opens; each part hides when unavailable.
StanceSection {
    id: quick

    required property StanceService service
    // "night" | "brightness" | "audio" | "" (all)
    property string part: ""
    readonly property var q: service.quick
    readonly property var night: q ? q.night : null
    readonly property var displays: q ? q.brightness : []
    readonly property var sinks: q ? q.sinks.filter(s => !s.virtual || s.default) : []
    readonly property bool busy: service.quickBusy
    readonly property bool compact: service.prefs.compact === true
    function shows(key) { return service.featureOn("quick") && (part === "" || part === key); }
    readonly property var partWidget: StanceWidgets.byId(part)

    icon: partWidget ? partWidget.icon : "tune"
    title: StanceI18n.t(partWidget ? partWidget.title : "Quick")
    visible: q !== null && ((shows("night") && night !== null) || (shows("brightness") && displays.length > 0) || (shows("audio") && sinks.length > 0))

    function sinkIcon(s) {
        if (s.virtual)
            return "graphic_eq";
        if (/hdmi|displayport/i.test(s.name + " " + s.profile))
            return "tv";
        if (s.bus === "usb" || s.bus === "bluetooth")
            return "headphones";
        return "speaker";
    }

    // ---- night light ---------------------------------------------------
    RowLayout {
        visible: quick.night !== null && quick.shows("night")
        Layout.fillWidth: true
        spacing: 8
        Text {
            // The card title says it when the card is only night light.
            visible: quick.part === ""
            text: StanceI18n.t("Night")
            color: quick.theme.textDim
            font.family: quick.theme.fontFamily
            font.pixelSize: quick.theme.fontSize - 1
            Layout.preferredWidth: 78
        }
        StanceSegmented {
            Layout.fillWidth: true
            theme: quick.theme
            enabled: !quick.busy
            model: [{ label: StanceI18n.t("Off"), value: 0 }, { label: "3.5k", value: 3500 }, { label: "4.5k", value: 4500 }, { label: "5.5k", value: 5500 }]
            current: quick.night && quick.night.on ? quick.night.temperature : 0
            onPicked: v => quick.service.runQuick(v === 0 ? ["night", "off"] : ["night", "set", String(v)])
        }
    }

    // ---- brightness ----------------------------------------------------
    Repeater {
        model: quick.shows("brightness") ? quick.displays : []
        delegate: RowLayout {
            required property var modelData
            Layout.fillWidth: true
            spacing: 8
            Row {
                Layout.preferredWidth: 78
                spacing: 5
                StanceIcon {
                    theme: quick.theme
                    name: modelData.kind === "backlight" ? "brightness_medium" : "light_mode"
                    size: quick.theme.fontSize + 3
                    color: quick.theme.textDim
                    anchors.verticalCenter: parent.verticalCenter
                }
                Text {
                    text: modelData.monitor
                    color: quick.theme.textDim
                    font.family: quick.theme.fontFamily
                    font.pixelSize: quick.theme.fontSize - 1
                    elide: Text.ElideRight
                    width: parent.width - x
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
            StanceSlider {
                id: sl
                theme: quick.theme
                Layout.fillWidth: true
                value: modelData.percent
                enabled: !quick.busy
                onCommitted: v => quick.service.runQuick(["brightness", modelData.monitor, String(v)])
            }
            Text {
                text: Math.round(sl.shown) + "%"
                color: quick.theme.text
                font.family: quick.theme.fontFamily
                font.pixelSize: quick.theme.fontSize - 1
                horizontalAlignment: Text.AlignRight
                Layout.preferredWidth: 36
            }
        }
    }

    // ---- audio output --------------------------------------------------
    ColumnLayout {
        visible: quick.sinks.length > 1 && quick.shows("audio")
        Layout.fillWidth: true
        spacing: 4
        Text {
            visible: quick.part === ""
            text: StanceI18n.t("Output")
            color: quick.theme.textDim
            font.family: quick.theme.fontFamily
            font.pixelSize: quick.theme.fontSize - 1
            Layout.topMargin: 2
        }
        Repeater {
            model: quick.sinks
            delegate: Rectangle {
                id: row
                required property var modelData
                readonly property bool on: modelData.default
                Layout.fillWidth: true
                implicitHeight: quick.compact ? 30 : 36
                radius: quick.theme.chipRadius
                opacity: quick.busy ? 0.6 : 1
                color: on ? quick.theme.accentSoft : (ma.containsMouse ? quick.theme.surface : "transparent")
                Behavior on color { ColorAnimation { duration: quick.theme.animMs } }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 10
                    anchors.rightMargin: 12
                    spacing: 8
                    StanceIcon {
                        theme: quick.theme
                        name: quick.sinkIcon(row.modelData)
                        size: quick.theme.fontSize + 3
                        fill: row.on
                        color: row.on ? quick.theme.accent : quick.theme.textDim
                    }
                    Text {
                        text: row.modelData.short
                        color: quick.theme.text
                        font.family: quick.theme.fontFamily
                        font.pixelSize: quick.theme.fontSize
                        font.weight: row.on ? Font.Medium : Font.Normal
                        elide: Text.ElideRight
                        Layout.maximumWidth: parent.width * 0.62
                    }
                    Text {
                        text: row.modelData.profile
                        visible: text !== "" && !quick.compact
                        color: quick.theme.textDim
                        font.family: quick.theme.fontFamily
                        font.pixelSize: quick.theme.fontSize - 2
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Item { Layout.fillWidth: !(row.modelData.profile !== "") }
                    // Radio dot, drawn so it needs no icon font.
                    Rectangle {
                        implicitWidth: 16
                        implicitHeight: 16
                        radius: 8
                        color: "transparent"
                        border.width: 2
                        border.color: row.on ? quick.theme.accent : quick.theme.textDim
                        Rectangle {
                            anchors.centerIn: parent
                            width: 8
                            height: 8
                            radius: 4
                            color: quick.theme.accent
                            visible: row.on
                        }
                    }
                }
                MouseArea {
                    id: ma
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: if (!row.on && !quick.busy) quick.service.runQuick(["audio", "set", String(row.modelData.id)])
                }
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
