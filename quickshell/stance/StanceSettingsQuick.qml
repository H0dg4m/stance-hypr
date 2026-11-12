import QtQuick
import QtQuick.Layouts

// Settings page: compact quick controls.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

    component PageTitle: Text {
        textFormat: Text.PlainText
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize + 2
        font.weight: Font.DemiBold
        Layout.fillWidth: true
        Layout.bottomMargin: 2
    }

    component Note: Text {
        textFormat: Text.PlainText
        color: page.theme.textDim
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize - 2
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    component Body: Text {
        textFormat: Text.PlainText
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize
    }

    PageTitle { text: StanceI18n.t("Quick settings") }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "density_small"
        title: StanceI18n.t("Quick controls")
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Note { text: StanceI18n.t("Choose what the panel shows in Layout."); Layout.fillWidth: true }
            StanceChip {
                theme: page.theme
                icon: "dashboard_customize"
                label: StanceI18n.t("Open Layout")
                onClicked: page.service.setPref("settingsPage", "layout")
            }
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            StanceIcon { theme: page.theme; name: "density_small"; size: page.theme.fontSize + 3; color: page.theme.textDim }
            Body { text: StanceI18n.t("Compact quick controls"); Layout.fillWidth: true }
            StanceSwitch {
                theme: page.theme
                checked: page.prefs.compact === true
                onToggled: v => page.service.setPref("compact", v)
            }
        }
    }
}
