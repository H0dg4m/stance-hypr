import QtQuick
import QtQuick.Layouts

// Settings page: feature switches.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

    component Dim: Text {
        textFormat: Text.PlainText
        color: page.theme.textDim
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize - 1
    }

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
    }

    PageTitle { text: StanceI18n.t("Features") }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "extension"
        Repeater {
            model: [
                { name: "clip", label: "Clipboard shelf", hint: "History and the shelf", icon: "content_paste" },
                { name: "deck", label: "Phone deck", hint: "Control the PC from a phone", icon: "smartphone" },
                { name: "rules", label: "Hotplug rules", hint: "Apply modes when screens are plugged in", icon: "usb" },
                { name: "quick", label: "Quick controls", hint: "Night light, brightness and audio in the panel", icon: "tune" }
            ]
            delegate: RowLayout {
                id: frow2
                required property var modelData
                readonly property var info: page.service.featureList.find(f => f.name === modelData.name) || null
                Layout.fillWidth: true
                spacing: 10
                StanceIcon { theme: page.theme; name: frow2.modelData.icon; size: page.theme.fontSize + 3; color: page.theme.textDim; Layout.alignment: Qt.AlignTop }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Body { text: StanceI18n.t(frow2.modelData.label); Layout.fillWidth: true }
                    Dim {
                        text: StanceI18n.t(frow2.modelData.hint)
                        font.pixelSize: page.theme.fontSize - 3
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    Dim {
                        visible: page.service.featureOn(frow2.modelData.name) && frow2.info !== null && frow2.info.unit_active === false
                        text: StanceI18n.t("Its background service is not running.")
                        color: page.theme.danger
                        font.pixelSize: page.theme.fontSize - 3
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                }
                StanceSwitch {
                    theme: page.theme
                    checked: page.service.featureOn(frow2.modelData.name)
                    onToggled: v => page.service.setFeature(frow2.modelData.name, v)
                }
            }
        }
        Dim {
            visible: page.service.featureError !== ""
            text: page.service.featureError
            color: page.theme.danger
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Note { text: StanceI18n.t("Turning a feature off also stops its background service; nothing stored is deleted.") }
    }
}
