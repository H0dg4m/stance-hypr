import QtQuick
import QtQuick.Layouts

// Panel settings: shows the page of the picked category. Each page lives in
// its own StanceSettings<Name>.qml and loads its own state.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    signal back
    // Category shown: general | layout | appearance | displays | quick | clip | phone | features | health
    property string cat: "general"

    spacing: 10

    StanceSettingsGeneral { visible: page.cat === "general"; service: page.service; theme: page.theme }
    StanceSettingsLayout { visible: page.cat === "layout"; service: page.service; theme: page.theme }
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
    StanceSettingsDisplays { visible: page.cat === "displays"; service: page.service; theme: page.theme }
    StanceSettingsQuick { visible: page.cat === "quick"; service: page.service; theme: page.theme }
    StanceSettingsClip { visible: page.cat === "clip"; service: page.service; theme: page.theme }
    StanceSettingsPhone { visible: page.cat === "phone"; service: page.service; theme: page.theme }
    StanceSettingsFeatures { visible: page.cat === "features"; service: page.service; theme: page.theme }
    StanceSettingsHealth { visible: page.cat === "health"; service: page.service; theme: page.theme }
}
