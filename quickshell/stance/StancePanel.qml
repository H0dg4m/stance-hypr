import QtQuick
import QtQuick.Layouts

// Panel content: a clickable map of the screen layout, settings for the
// selected screen, and modes. Every click is one `stance …` call; the
// confirm overlay takes over from there.
ColumnLayout {
    id: panel

    required property StanceService service
    property StanceTheme theme: StanceTheme {}
    property string selectedName: ""
    // Screen the panel was opened on; selected until the user picks another.
    property string preferredName: ""
    // "main" | "mode:<name>"
    property string page: "main"
    // Settings live in their own window (StanceSettingsWindow).
    signal settingsRequested
    // The clipboard widget asks the shell to open the shelf.
    signal clipRequested
    // false where no clipboard shelf answers (ii): the clip widget hides.
    property bool hasClipShelf: true

    readonly property var selected: {
        const ms = service.monitors;
        return ms.find(m => m.name === selectedName) || ms.find(m => m.name === preferredName)
            || ms.find(m => m.focused) || ms[0] || null;
    }

    spacing: 10
    width: 380

    function fmtHz(r) { return (Math.round(r * 100) / 100).toString(); }

    function logical(m) {
        const rot = m.transform % 2 === 1;
        const s = m.scale > 0 ? m.scale : 1;
        return { w: (rot ? m.height : m.width) / s, h: (rot ? m.width : m.height) / s };
    }

    function refreshRates(m) {
        const seen = {};
        const out = [];
        for (const md of m.modes) {
            if (md.width !== m.width || md.height !== m.height) continue;
            const k = Math.round(md.refresh);
            if (seen[k]) continue;
            seen[k] = true;
            out.push(md.refresh);
        }
        return out.sort((a, b) => b - a).slice(0, 6);
    }

    function resolutions(m) {
        const seen = {};
        const out = [];
        for (const md of m.modes) {
            const k = md.width + "x" + md.height;
            if (seen[k]) continue;
            seen[k] = true;
            out.push({ w: md.width, h: md.height });
        }
        out.sort((a, b) => b.w * b.h - a.w * a.h);
        const top = out.slice(0, 4);
        if (m.enabled && !top.some(r => r.w === m.width && r.h === m.height))
            top.push({ w: m.width, h: m.height });
        return top;
    }

    function others(m) {
        return service.monitors.filter(o => o.name !== m.name && o.enabled && !o.mirror_of);
    }

    function activeCount() {
        return service.monitors.filter(o => o.enabled && !o.mirror_of).length;
    }

    // ---- header ----------------------------------------------------------
    RowLayout {
        Layout.fillWidth: true
        Layout.leftMargin: 4
        Layout.rightMargin: 2
        spacing: 8
        StanceChip {
            visible: panel.page !== "main"
            theme: panel.theme
            icon: "arrow_back"
            label: panel.theme.iconFont === "" ? StanceI18n.t("Back") : ""
            // The mode editor may hold unsaved edits and ask first.
            onClicked: editorLoader.item && panel.page.startsWith("mode:") ? editorLoader.item.requestBack() : panel.page = "main"
        }
        StanceIcon { visible: panel.page === "main" && theme.iconFont !== ""; theme: panel.theme; name: "desktop_windows"; color: panel.theme.accent; fill: true }
        Text {
            textFormat: Text.PlainText
            text: panel.page.startsWith("mode:") ? StanceI18n.f("Mode · %1", panel.page.slice(5)) : StanceI18n.t("Displays")
            color: panel.theme.text
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize + 3
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }
        Rectangle {
            visible: panel.service.activeMode !== ""
            radius: height / 2
            color: panel.theme.accentSoft
            implicitHeight: 24
            implicitWidth: modeRow.implicitWidth + 16
            Row {
                id: modeRow
                anchors.centerIn: parent
                spacing: 4
                StanceIcon { theme: panel.theme; name: "bookmark"; size: panel.theme.fontSize + 1; color: panel.theme.accent; fill: true; anchors.verticalCenter: parent.verticalCenter }
                Text {
                    textFormat: Text.PlainText
                    text: panel.service.activeMode
                    color: panel.theme.accent
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize - 2
                    font.weight: Font.Medium
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
        }
        StanceChip {
            visible: panel.page === "main" && panel.service.monitors.length > 1
            theme: panel.theme
            icon: "screen_search_desktop"
            label: panel.theme.iconFont === "" ? StanceI18n.t("Identify") : ""
            onClicked: identify.active = true
        }
        StanceChip {
            visible: panel.page === "main"
            theme: panel.theme
            icon: "settings"
            label: panel.theme.iconFont === "" ? StanceI18n.t("Settings") : ""
            onClicked: panel.settingsRequested()
        }
    }

    ColumnLayout {
    id: mainPage
    visible: panel.page === "main"
    Layout.fillWidth: true
    spacing: 10

    StanceIdentify {
        id: identify
        theme: panel.theme
        monitors: panel.service.monitors
    }

    StanceSetup {
        visible: !panel.service.prefs.onboarded
        Layout.fillWidth: true
        service: panel.service
        theme: panel.theme
        onCustomize: panel.settingsRequested()
    }

    StanceBoard {
        Layout.fillWidth: true
        host: panel
        onEditMode: name => panel.page = "mode:" + name
        onClipRequested: panel.clipRequested()
        onSettingsRequested: cat => {
            panel.service.setPref("settingsPage", cat);
            panel.settingsRequested();
        }
    }


    }

    Loader {
        id: editorLoader
        Layout.fillWidth: true
        active: panel.page.startsWith("mode:")
        visible: active
        sourceComponent: StanceModeEditor {
            service: panel.service
            theme: panel.theme
            modeName: panel.page.slice(5)
            onBack: panel.page = "main"
        }
    }

    // ---- messages --------------------------------------------------------
    Rectangle {
        visible: panel.service.lastError !== "" || panel.service.lastNote !== ""
        Layout.fillWidth: true
        implicitHeight: msg.implicitHeight + 16
        radius: panel.theme.chipRadius
        readonly property bool isError: panel.service.lastError !== ""
        color: isError ? Qt.rgba(panel.theme.danger.r, panel.theme.danger.g, panel.theme.danger.b, 0.14) : panel.theme.card
        RowLayout {
            id: msg
            x: 10; y: 8
            width: parent.width - 20
            spacing: 8
            StanceIcon {
                theme: panel.theme
                name: parent.parent.isError ? "error" : "info"
                size: panel.theme.fontSize + 2
                color: parent.parent.isError ? panel.theme.danger : panel.theme.textDim
                Layout.alignment: Qt.AlignTop
            }
            Text {
                textFormat: Text.PlainText
                text: panel.service.lastError || panel.service.lastNote
                color: parent.parent.isError ? panel.theme.danger : panel.theme.textDim
                font.family: panel.theme.fontFamily
                font.pixelSize: panel.theme.fontSize - 2
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
}
