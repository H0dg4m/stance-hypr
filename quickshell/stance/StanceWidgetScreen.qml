import QtQuick
import QtQuick.Layouts

// Widget "screen": settings for the screen selected on the map (or the one
// the panel opened on). `panel` is the StancePanel.
ColumnLayout {
    id: screenWidget
    required property var panel
    property string size: "L"
    spacing: 0

    component RowLabel: Text {
        required property StanceTheme theme
        color: theme.textDim
        font.family: theme.fontFamily
        font.pixelSize: theme.fontSize - 1
        Layout.preferredWidth: 78
    }

    // ---- selected screen -------------------------------------------------
    StanceSection {
        id: detail
        visible: panel.selected !== null
        Layout.fillWidth: true
        theme: panel.theme
        readonly property var m: panel.selected
        readonly property var ref: m ? panel.others(m)[0] : null
        readonly property bool active: m && m.enabled && !m.mirror_of

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            StanceIcon { theme: panel.theme; name: "monitor"; color: panel.theme.textDim }
            ColumnLayout {
                spacing: 0
                Layout.fillWidth: true
                Layout.preferredWidth: 1000
                Text {
                    textFormat: Text.PlainText
                    text: detail.m ? detail.m.name : ""
                    color: panel.theme.text
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize + 1
                    font.weight: Font.DemiBold
                }
                Text {
                    textFormat: Text.PlainText
                    text: detail.m ? detail.m.description : ""
                    visible: text !== ""
                    elide: Text.ElideRight
                    color: panel.theme.textDim
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize - 2
                    Layout.fillWidth: true
                }
            }
            StanceChip {
                theme: panel.theme
                icon: "power_settings_new"
                label: detail.m && detail.m.enabled ? StanceI18n.t("Turn off") : StanceI18n.t("Turn on")
                danger: detail.m && detail.m.enabled
                enabled: detail.m && !panel.service.busy && !(detail.active && panel.activeCount() <= 1)
                onClicked: panel.service.run(["set", detail.m.name, detail.m.enabled ? "--off" : "--on"])
            }
        }

        StanceChip {
            visible: detail.m && detail.m.enabled && !!detail.m.mirror_of
            theme: panel.theme
            icon: "close"
            label: StanceI18n.f("Stop mirroring %1", detail.m && detail.m.mirror_of ? detail.m.mirror_of : "")
            enabled: !panel.service.busy
            onClicked: panel.service.run(["layout", "right-of", detail.m.name, detail.m.mirror_of])
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: StanceI18n.t("Refresh") }
            StanceSegmented {
                Layout.fillWidth: true
                theme: panel.theme
                enabled: !panel.service.busy
                model: detail.active ? panel.refreshRates(detail.m).map(r => ({ label: panel.fmtHz(r), value: r })) : []
                current: detail.m ? detail.m.refresh : null
                same: (a, b) => Math.abs(a - b) < 0.5
                onPicked: v => panel.service.run(["set", detail.m.name, "--hz", String(v)])
            }
            Text {
                textFormat: Text.PlainText
                text: "Hz"
                color: panel.theme.textDim
                font.family: panel.theme.fontFamily
                font.pixelSize: panel.theme.fontSize - 2
            }
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: StanceI18n.t("Scale") }
            StanceSegmented {
                Layout.fillWidth: true
                theme: panel.theme
                enabled: !panel.service.busy
                model: [1, 1.25, 1.5, 2].map(v => ({ label: v + "×", value: v }))
                current: detail.m ? detail.m.scale : null
                same: (a, b) => Math.abs(a - b) < 0.05
                onPicked: v => panel.service.run(["set", detail.m.name, "--scale", String(v)])
            }
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            RowLabel { theme: panel.theme; text: StanceI18n.t("Resolution"); Layout.alignment: Qt.AlignTop; Layout.topMargin: 6 }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: detail.active ? panel.resolutions(detail.m) : []
                    delegate: StanceChip {
                        required property var modelData
                        theme: panel.theme
                        label: modelData.w + "×" + modelData.h
                        selected: modelData.w === detail.m.width && modelData.h === detail.m.height
                        enabled: !panel.service.busy
                        onClicked: if (!selected) panel.service.run(["set", detail.m.name, "--res", modelData.w + "x" + modelData.h])
                    }
                }
            }
        }

        RowLayout {
            visible: detail.active && !!detail.ref
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: StanceI18n.t("Place"); Layout.alignment: Qt.AlignTop; Layout.topMargin: 6 }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: [
                        { label: StanceI18n.t("Left"), icon: "west", args: ["layout", "left-of"] },
                        { label: StanceI18n.t("Right"), icon: "east", args: ["layout", "right-of"] },
                        { label: StanceI18n.t("Above"), icon: "north", args: ["layout", "above"] },
                        { label: StanceI18n.t("Mirror"), icon: "content_copy", args: ["layout", "mirror"] }
                    ]
                    delegate: StanceChip {
                        required property var modelData
                        theme: panel.theme
                        icon: modelData.icon
                        label: modelData.label
                        enabled: !panel.service.busy
                        onClicked: panel.service.run(modelData.args.concat([detail.m.name, detail.ref.name]))
                    }
                }
                StanceChip {
                    theme: panel.theme
                    icon: "crop_square"
                    label: StanceI18n.t("Only this")
                    enabled: !panel.service.busy
                    onClicked: panel.service.run(["layout", "single", detail.m.name])
                }
            }
        }
        Text {
            textFormat: Text.PlainText
            visible: detail.active && !!detail.ref
            text: StanceI18n.f("relative to %1", detail.ref ? detail.ref.name : "")
            color: panel.theme.textDim
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize - 3
            Layout.leftMargin: 84
            Layout.topMargin: -4
        }
    }
}
