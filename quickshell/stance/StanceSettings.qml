import QtQuick
import QtQuick.Layouts

// Panel settings: shows the page of the picked category. Each page lives in
// its own StanceSettings<Name>.qml and loads its own state; only the shown
// one is built, so opening Settings does not run every page's checks.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    signal back
    // Category shown: general | layout | appearance | displays | quick | clip | phone | features | health
    property string cat: "general"

    spacing: 10

    Loader {
        active: page.cat === "general"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsGeneral { service: page.service; theme: page.theme }
    }
    Loader {
        active: page.cat === "layout"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsLayout { service: page.service; theme: page.theme }
    }
    ColumnLayout {
        visible: page.cat === "appearance"
        Layout.fillWidth: true
        spacing: 10
        Text {
            text: StanceI18n.t("Appearance")
            color: page.theme.text
            font.family: page.theme.fontFamily
            font.pixelSize: page.theme.fontSize + 2
            font.weight: Font.DemiBold
            Layout.fillWidth: true
            Layout.bottomMargin: 2
        }
        StanceAppearance {
            Layout.fillWidth: true
            service: page.service
            theme: page.theme
        }
    }
    Loader {
        active: page.cat === "displays"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsDisplays { service: page.service; theme: page.theme }
    }
    Loader {
        active: page.cat === "quick"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsQuick { service: page.service; theme: page.theme }
    }
    Loader {
        active: page.cat === "clip"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsClip { service: page.service; theme: page.theme }
    }
    Loader {
        active: page.cat === "phone"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsPhone { service: page.service; theme: page.theme }
    }
    Loader {
        active: page.cat === "features"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsFeatures { service: page.service; theme: page.theme }
    }
    Loader {
        active: page.cat === "health"
        visible: active
        Layout.fillWidth: true
        sourceComponent: StanceSettingsHealth { service: page.service; theme: page.theme }
    }
}
