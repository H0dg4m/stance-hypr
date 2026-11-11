import QtQuick
import QtQuick.Layouts

// Settings page: what the panel shows, in which order and size (the widget
// board, docs/widgets.md).
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme

    Layout.fillWidth: true
    spacing: 10

    readonly property var board: service.prefs.board || []
    readonly property var hidden: StanceWidgets.list.filter(w => !board.some(e => e.id === w.id))
    readonly property var sizeNames: ({ "S": StanceI18n.t("Small"), "M": StanceI18n.t("Medium"), "L": StanceI18n.t("Large") })

    component Note: Text {
        color: page.theme.textDim
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize - 2
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    component PageTitle: Text {
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize + 2
        font.weight: Font.DemiBold
        Layout.fillWidth: true
        Layout.bottomMargin: 2
    }

    component Body: Text {
        textFormat: Text.PlainText
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize
        elide: Text.ElideRight
    }

    PageTitle { text: StanceI18n.t("Layout") }

    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "auto_awesome_mosaic"
        title: StanceI18n.t("Presets")
        Flow {
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: StanceWidgets.presetNames
                delegate: StanceChip {
                    required property string modelData
                    theme: page.theme
                    label: modelData === "minimal" ? StanceI18n.t("Minimal") : modelData === "standard" ? StanceI18n.t("Standard") : StanceI18n.t("Everything")
                    onClicked: page.service.setBoard(StanceWidgets.preset(modelData))
                }
            }
            StanceChip {
                theme: page.theme
                icon: "restart_alt"
                label: StanceI18n.t("Reset layout")
                onClicked: page.service.resetBoard()
            }
        }
    }

    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "dashboard_customize"
        title: StanceI18n.t("On the panel")
        Note {
            visible: page.board.length === 0
            text: StanceI18n.t("Nothing on the panel yet.")
        }
        Repeater {
            model: page.board
            delegate: RowLayout {
                id: row
                required property var modelData
                required property int index
                readonly property var w: StanceWidgets.byId(modelData.id)
                readonly property bool off: w !== null && w.feature !== null && !page.service.featureOn(w.feature)
                Layout.fillWidth: true
                spacing: 8
                StanceIcon { theme: page.theme; name: row.w ? row.w.icon : ""; size: page.theme.fontSize + 3; color: page.theme.textDim }
                Body { text: StanceI18n.t(row.w ? row.w.title : row.modelData.id); Layout.fillWidth: true; Layout.minimumWidth: 40 }
                Note {
                    visible: row.off
                    text: StanceI18n.t("Off in Features")
                    Layout.fillWidth: false
                }
                StanceSegmented {
                    visible: row.w !== null && row.w.sizes.length > 1
                    theme: page.theme
                    model: row.w ? row.w.sizes.map(s => ({ label: page.sizeNames[s], value: s })) : []
                    current: row.modelData.size
                    onPicked: v => page.service.setWidget(row.modelData.id, v)
                }
                StanceChip {
                    theme: page.theme
                    icon: "arrow_upward"
                    label: page.theme.iconFont === "" ? "\u2191" : ""
                    enabled: row.index > 0
                    onClicked: page.service.moveWidget(row.modelData.id, -1)
                }
                StanceChip {
                    theme: page.theme
                    icon: "arrow_downward"
                    label: page.theme.iconFont === "" ? "\u2193" : ""
                    enabled: row.index < page.board.length - 1
                    onClicked: page.service.moveWidget(row.modelData.id, 1)
                }
                StanceChip {
                    theme: page.theme
                    icon: "close"
                    label: page.theme.iconFont === "" ? "\u00d7" : ""
                    danger: true
                    onClicked: page.service.setWidget(row.modelData.id, "")
                }
            }
        }
    }

    StanceSection {
        visible: page.hidden.length > 0
        Layout.fillWidth: true
        theme: page.theme
        icon: "visibility_off"
        title: StanceI18n.t("Not on the panel")
        Repeater {
            model: page.hidden
            delegate: RowLayout {
                id: hrow
                required property var modelData
                Layout.fillWidth: true
                spacing: 8
                StanceIcon { theme: page.theme; name: hrow.modelData.icon; size: page.theme.fontSize + 3; color: page.theme.textDim }
                Body { text: StanceI18n.t(hrow.modelData.title); Layout.fillWidth: true }
                StanceChip {
                    theme: page.theme
                    icon: "add"
                    label: StanceI18n.t("Add")
                    onClicked: page.service.setWidget(hrow.modelData.id, hrow.modelData.defaultSize)
                }
            }
        }
    }
}
