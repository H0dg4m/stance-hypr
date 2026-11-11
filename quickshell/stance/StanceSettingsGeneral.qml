import QtQuick
import QtQuick.Layouts

// Settings page: language, layout link and behaviour.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

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

    PageTitle { text: StanceI18n.t("General") }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "translate"
        title: StanceI18n.t("Language")
        StanceSegmented {
            Layout.fillWidth: true
            theme: page.theme
            model: [{ label: StanceI18n.t("Auto"), value: "auto" }, { label: "English", value: "en" }, { label: "Türkçe", value: "tr" }]
            current: page.prefs.lang
            onPicked: v => page.service.setPref("lang", v)
        }
    }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "dashboard_customize"
        title: StanceI18n.t("Panel layout")
        Note { text: StanceI18n.t("Choose what the panel shows in Layout.") }
        Flow {
            Layout.fillWidth: true
            spacing: 6
            StanceChip {
                theme: page.theme
                icon: "dashboard_customize"
                label: StanceI18n.t("Open Layout")
                onClicked: page.service.setPref("settingsPage", "layout")
            }
            StanceChip {
                theme: page.theme
                icon: "restart_alt"
                label: StanceI18n.t("Run setup again")
                onClicked: page.service.rerunSetup()
            }
        }
    }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "tune"
        title: StanceI18n.t("Behaviour")
        trailing: [
            StanceChip {
                theme: page.theme
                icon: "restart_alt"
                label: StanceI18n.t("Reset panel settings")
                onClicked: page.service.resetPrefs()
            }
        ]
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            RowLayout {
                Layout.fillWidth: true
                Body { text: StanceI18n.t("Confirm window"); Layout.fillWidth: true }
                Body { text: Math.round(cs.shown) + " s" }
            }
            StanceSlider {
                id: cs
                Layout.fillWidth: true
                theme: page.theme
                from: 5
                to: 60
                value: page.service.confirmTimeout
                onCommitted: v => page.service.setPref("confirmTimeout", Math.round(v))
            }
            Note { text: StanceI18n.t("Display changes revert on their own after this time unless you keep them.") }
        }
    }
}
