import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

// Settings page: health checks and about.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

    property string version: ""
    property var checks: []
    property bool showAllChecks: false
    readonly property var notes: checks.filter(c => c.status !== "ok")
    function loadChecks() {
        service.doctor(r => {
            if (r && r.ok)
                checks = r.data;
        });
    }
    function statusIcon(s) {
        return s === "ok" ? "check_circle" : s === "info" ? "info" : s === "warn" ? "warning" : "error";
    }
    function statusColor(s) {
        return s === "ok" ? theme.accent : s === "fail" ? theme.danger : theme.textDim;
    }

    Component.onCompleted: {
        loadChecks();
        verProc.running = true;
    }

    Process {
        id: verProc
        command: [page.service.bin, "--version"]
        stdout: StdioCollector { onStreamFinished: page.version = text.trim() }
    }

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

    PageTitle { text: StanceI18n.t("Health & about") }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "health_and_safety"
        title: StanceI18n.t("Health checks")
        trailing: [
            StanceChip {
                theme: page.theme
                icon: page.showAllChecks ? "expand_less" : "expand_more"
                label: page.showAllChecks ? StanceI18n.t("Notes") : StanceI18n.t("All")
                onClicked: page.showAllChecks = !page.showAllChecks
            }
        ]
        Dim {
            visible: page.checks.length === 0
            text: StanceI18n.t("Checking…")
        }
        Dim {
            visible: page.checks.length > 0
            text: page.notes.length ? StanceI18n.f("%1 checks ok, %2 to look at", page.checks.length - page.notes.length, page.notes.length) : StanceI18n.f("%1 checks ok", page.checks.length)
        }
        Repeater {
            model: page.showAllChecks ? page.checks : page.notes
            delegate: RowLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: 8
                StanceIcon {
                    theme: page.theme
                    name: page.statusIcon(modelData.status)
                    size: page.theme.fontSize + 3
                    color: page.statusColor(modelData.status)
                    Layout.alignment: Qt.AlignTop
                }
                Dim {
                    text: modelData.message
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                    font.pixelSize: page.theme.fontSize - 2
                }
            }
        }
    }

    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "info"
        title: StanceI18n.t("About")
        Body { text: page.version !== "" ? page.version : "stance"; Layout.fillWidth: true }
        Note { text: StanceI18n.t("Display modes, hotplug rules, night light, audio and a clipboard shelf for Hyprland.") }
        Flow {
            Layout.fillWidth: true
            spacing: 6
            StanceChip { theme: page.theme; icon: "open_in_new"; label: StanceI18n.t("Source on GitHub"); onClicked: Qt.openUrlExternally("https://github.com/H0dg4m/stance-hypr") }
            StanceChip { theme: page.theme; icon: "bug_report"; label: StanceI18n.t("Report an issue"); onClicked: Qt.openUrlExternally("https://github.com/H0dg4m/stance-hypr/issues") }
        }
    }
}
