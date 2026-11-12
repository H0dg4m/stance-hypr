import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

// Panel settings in their own window, centred on `screen`: category navigation
// (sidebar, or a top row on narrow screens) and one scrolling page at a time.
// The picked category is remembered in panel.json (`settingsPage`). Esc or the
// close chip shut the window.
LazyLoader {
    id: root

    required property StanceService service
    property StanceTheme theme: StanceTheme {}
    property var screen: null
    property bool open: false

    active: open

    component: PanelWindow {
        id: win
        screen: root.screen
        color: "transparent"
        // No anchors: the compositor centres the surface.
        implicitWidth: frame.implicitWidth
        implicitHeight: frame.implicitHeight
        exclusionMode: ExclusionMode.Ignore
        exclusiveZone: 0
        WlrLayershell.namespace: "stance:settings"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

        readonly property real maxHeight: (win.screen ? win.screen.height : 1080) * 0.85
        readonly property real screenW: win.screen ? win.screen.width : 1920
        readonly property bool narrow: screenW < 860
        readonly property var categories: [
            { id: "general", label: StanceI18n.t("General"), icon: "settings" },
            { id: "layout", label: StanceI18n.t("Layout"), icon: "dashboard_customize" },
            { id: "appearance", label: StanceI18n.t("Appearance"), icon: "palette" },
            { id: "displays", label: StanceI18n.t("Displays & modes"), icon: "monitor" },
            { id: "quick", label: StanceI18n.t("Quick settings"), icon: "tune", feature: "quick" },
            { id: "clip", label: StanceI18n.t("Clipboard"), icon: "content_paste", feature: "clip" },
            { id: "phone", label: StanceI18n.t("Phone"), icon: "smartphone", feature: "deck" },
            { id: "features", label: StanceI18n.t("Features"), icon: "extension" },
            { id: "health", label: StanceI18n.t("Health & about"), icon: "health_and_safety" }
        ]
        // A category whose feature is off is hidden (Features always stays).
        readonly property var visibleCats: categories.filter(c => !c.feature || root.service.featureOn(c.feature))
        readonly property string cat: visibleCats.some(c => c.id === root.service.prefs.settingsPage) ? root.service.prefs.settingsPage : "general"

        Rectangle {
            id: frame
            implicitWidth: win.narrow ? Math.max(320, win.screenW - 32) : 780
            implicitHeight: Math.min(win.maxHeight, 660)
            radius: root.theme.radius
            color: root.theme.backgroundSolid
            border.width: 1
            border.color: root.theme.border
            focus: true
            Keys.onEscapePressed: root.open = false

            opacity: 0
            Component.onCompleted: fadeIn.start()
            NumberAnimation { id: fadeIn; target: frame; property: "opacity"; to: 1; duration: root.theme.animMs * 1.5; easing.type: Easing.OutCubic }

            RowLayout {
                id: header
                x: 16
                y: 12
                width: parent.width - 32
                spacing: 8
                StanceIcon { visible: root.theme.iconFont !== ""; theme: root.theme; name: "settings"; color: root.theme.accent; fill: true }
                Text {
                    textFormat: Text.PlainText
                    text: StanceI18n.t("Panel settings")
                    color: root.theme.text
                    font.family: root.theme.fontFamily
                    font.pixelSize: root.theme.fontSize + 3
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                }
                StanceChip {
                    theme: root.theme
                    icon: "close"
                    label: root.theme.iconFont === "" ? StanceI18n.t("Close") : ""
                    onClicked: root.open = false
                }
            }

            StanceNav {
                id: nav
                z: 10
                theme: root.theme
                model: win.visibleCats
                current: win.cat
                vertical: !win.narrow
                x: 16
                y: header.y + header.implicitHeight + 12
                width: win.narrow ? parent.width - 32 : 168
                height: win.narrow ? 38 : implicitHeight
                onPicked: id => {
                    root.service.setPref("settingsPage", id);
                    flick.contentY = 0;
                }
            }

            // Hairline between the sidebar and the page.
            Rectangle {
                visible: !win.narrow
                x: nav.x + nav.width + 8
                y: nav.y
                width: 1
                height: parent.height - y - 12
                color: root.theme.border
            }

            Flickable {
                id: flick
                x: win.narrow ? 16 : nav.x + nav.width + 20
                y: win.narrow ? nav.y + nav.height + 10 : nav.y
                width: parent.width - x - 16
                height: parent.height - y - 12
                clip: true
                contentWidth: width
                contentHeight: settings.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {
                    policy: flick.contentHeight > flick.height ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
                }

                StanceSettings {
                    id: settings
                    // Leave room for the scroll bar.
                    width: flick.width - 12
                    service: root.service
                    theme: root.theme
                    cat: win.cat
                    onBack: root.open = false
                }
            }
        }
    }
}
