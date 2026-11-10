import QtQuick
import QtQuick.Layouts

// Panel settings page: what the panel shows, how long a display change waits
// for confirmation, hotplug rules and a short health summary.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    signal back

    spacing: 10

    readonly property var prefs: service.prefs
    readonly property var toggles: [
        { key: "map", label: "Screen map", icon: "grid_view" },
        { key: "screen", label: "Screen settings", icon: "monitor" },
        { key: "night", label: "Night light", icon: "bedtime" },
        { key: "brightness", label: "Brightness", icon: "light_mode" },
        { key: "audio", label: "Audio output", icon: "volume_up" },
        { key: "modes", label: "Modes", icon: "bookmarks" }
    ]

    // ---- rules state ------------------------------------------------------
    property var rules: []
    property bool rulesLoaded: false
    property string rulesError: ""
    property bool addingRule: false
    property var newMonitors: []
    property string newMode: ""
    property bool newExtras: false

    // Stable match string for a connected monitor: its description when that
    // is unique, else the connector name.
    function matchOf(m) {
        const same = service.monitors.filter(x => x.description === m.description).length;
        return m.description && same === 1 ? m.description : m.name;
    }

    function loadRules() {
        rulesError = "";
        service.getRules(r => {
            if (r && r.ok) {
                rules = (r.data && r.data.rule) || [];
                rulesLoaded = true;
            } else {
                rulesError = r && r.error ? r.error.message : "stance returned no output";
            }
        });
    }

    function saveRules(next) {
        rulesError = "";
        service.putRules({ rule: next }, r => {
            if (r && r.ok) {
                rules = next;
                addingRule = false;
                newMonitors = [];
                newMode = "";
                newExtras = false;
            } else {
                rulesError = r && r.error ? r.error.message : "stance returned no output";
            }
        });
    }

    function addRule() {
        if (newMonitors.length === 0 || newMode === "")
            return;
        saveRules(rules.concat([{ monitors: newMonitors, mode: newMode, allow_extras: newExtras }]));
    }

    function removeRule(i) {
        saveRules(rules.filter((_, j) => j !== i));
    }

    function toggleNewMonitor(match) {
        newMonitors = newMonitors.indexOf(match) >= 0 ? newMonitors.filter(x => x !== match) : newMonitors.concat([match]);
    }

    // ---- health state -------------------------------------------------------
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
        service.call(["status"], r => { if (r && r.ok && r.data) service.daemon = r.data.daemon === true; });
        loadRules();
        loadChecks();
    }

    component Dim: Text {
        color: page.theme.textDim
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize - 1
    }
    component Body: Text {
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize
    }

    // ---- show in panel ----------------------------------------------------
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "visibility"
        title: "Show in panel"
        Repeater {
            model: page.toggles
            delegate: RowLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: 10
                StanceIcon { theme: page.theme; name: modelData.icon; size: page.theme.fontSize + 3; color: page.theme.textDim }
                Body { text: modelData.label; Layout.fillWidth: true }
                StanceSwitch {
                    theme: page.theme
                    checked: !(page.prefs.show && page.prefs.show[modelData.key] === false)
                    onToggled: v => page.service.setPref("show." + modelData.key, v)
                }
            }
        }
    }

    // ---- behaviour ----------------------------------------------------------
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "tune"
        title: "Behaviour"
        trailing: [
            StanceChip {
                theme: page.theme
                icon: "restart_alt"
                label: "Reset"
                onClicked: page.service.resetPrefs()
            }
        ]
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            RowLayout {
                Layout.fillWidth: true
                Body { text: "Confirm window"; Layout.fillWidth: true }
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
            Dim {
                text: "Display changes revert on their own after this time unless you keep them."
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                font.pixelSize: page.theme.fontSize - 2
            }
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Body { text: "Compact quick controls"; Layout.fillWidth: true }
            StanceSwitch {
                theme: page.theme
                checked: page.prefs.compact === true
                onToggled: v => page.service.setPref("compact", v)
            }
        }
    }

    // ---- hotplug rules --------------------------------------------------------
    StanceSection {
        id: rulesCard
        Layout.fillWidth: true
        theme: page.theme
        icon: "usb"
        title: "Hotplug rules"
        trailing: [
            StanceChip {
                theme: page.theme
                icon: page.addingRule ? "close" : "add"
                label: page.addingRule ? "Cancel" : "Add"
                enabled: page.rulesLoaded
                onClicked: page.addingRule = !page.addingRule
            }
        ]

        Dim {
            text: "When exactly these screens are connected, a mode is applied by the background service (`stance daemon`)."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            font.pixelSize: page.theme.fontSize - 2
        }

        TextEdit {
            visible: !page.service.daemon
            readOnly: true
            selectByMouse: true
            text: "Background service is off — rules and returning re-plugged screens to their last place need it: systemctl --user enable --now stance.service"
            wrapMode: TextEdit.Wrap
            Layout.fillWidth: true
            color: page.theme.textDim
            selectionColor: page.theme.accent
            selectedTextColor: page.theme.accentText
            font.family: page.theme.fontFamily
            font.pixelSize: page.theme.fontSize - 2
        }

        Repeater {
            model: page.rules
            delegate: Rectangle {
                id: rrow
                required property var modelData
                required property int index
                Layout.fillWidth: true
                implicitHeight: rcol.implicitHeight + 16
                radius: page.theme.chipRadius
                color: page.theme.surface
                RowLayout {
                    id: rcol
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 8
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Body {
                            text: rrow.modelData.monitors.join(" + ")
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                        Dim {
                            text: "→ " + rrow.modelData.mode + (rrow.modelData.allow_extras ? " · with night light, audio, brightness" : "")
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                            font.pixelSize: page.theme.fontSize - 2
                        }
                    }
                    StanceChip {
                        theme: page.theme
                        icon: "delete"
                        danger: true
                        implicitWidth: 34
                        enabled: !page.service.callBusy
                        onClicked: page.removeRule(rrow.index)
                    }
                }
            }
        }
        Dim {
            visible: page.rulesLoaded && page.rules.length === 0 && !page.addingRule
            text: "No rules. Rules apply a mode on their own when screens are plugged in."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            font.pixelSize: page.theme.fontSize - 2
        }

        // add form
        ColumnLayout {
            visible: page.addingRule
            Layout.fillWidth: true
            spacing: 8
            Dim { text: "When these screens are connected" }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: page.service.monitors
                    delegate: StanceChip {
                        required property var modelData
                        theme: page.theme
                        icon: "monitor"
                        label: modelData.name
                        tip: modelData.description
                        selected: page.newMonitors.indexOf(page.matchOf(modelData)) >= 0
                        onClicked: page.toggleNewMonitor(page.matchOf(modelData))
                    }
                }
            }
            Dim { text: "Apply this mode" }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: page.service.modes.filter(m => !m.error)
                    delegate: StanceChip {
                        required property var modelData
                        theme: page.theme
                        icon: "bookmark"
                        label: modelData.name
                        selected: page.newMode === modelData.name
                        onClicked: page.newMode = modelData.name
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                StanceSwitch { theme: page.theme; checked: page.newExtras; onToggled: v => page.newExtras = v }
                Body {
                    text: "Also set night light, audio and brightness"
                    font.pixelSize: page.theme.fontSize - 1
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
            StanceChip {
                Layout.fillWidth: true
                Layout.preferredHeight: 36
                theme: page.theme
                icon: "check"
                label: page.service.callBusy ? "Saving…" : "Add rule"
                selected: true
                enabled: page.newMonitors.length > 0 && page.newMode !== "" && !page.service.callBusy
                onClicked: page.addRule()
            }
        }
        Text {
            visible: page.rulesError !== ""
            text: page.rulesError
            color: page.theme.danger
            font.family: page.theme.fontFamily
            font.pixelSize: page.theme.fontSize - 1
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }

    // ---- health ---------------------------------------------------------------
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "health_and_safety"
        title: "Health"
        trailing: [
            StanceChip {
                theme: page.theme
                icon: page.showAllChecks ? "expand_less" : "expand_more"
                label: page.showAllChecks ? "Notes" : "All"
                onClicked: page.showAllChecks = !page.showAllChecks
            }
        ]
        Dim {
            visible: page.checks.length === 0
            text: "Checking…"
        }
        Dim {
            visible: page.checks.length > 0
            text: (page.checks.length - page.notes.length) + " checks ok" + (page.notes.length ? ", " + page.notes.length + " to look at" : "")
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
}
