import QtQuick
import QtQuick.Layouts

// Settings page: saved modes and hotplug rules.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

    // ---- rules state ------------------------------------------------------
    property var rules: []
    property bool rulesLoaded: false
    property string rulesError: ""
    property bool addingRule: false
    property var newMonitors: []
    property string newMode: ""
    property string newPower: ""      // "" | "ac" | "battery"
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
                rulesError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
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
                newPower = "";
            } else {
                rulesError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }

    function addRule() {
        if (newMonitors.length === 0 || newMode === "")
            return;
        const r = { monitors: newMonitors, mode: newMode, allow_extras: newExtras };
        if (newPower !== "")
            r.on_power = newPower;
        saveRules(rules.concat([r]));
    }

    function removeRule(i) {
        saveRules(rules.filter((_, j) => j !== i));
    }

    function toggleNewMonitor(match) {
        newMonitors = newMonitors.indexOf(match) >= 0 ? newMonitors.filter(x => x !== match) : newMonitors.concat([match]);
    }

    Component.onCompleted: {
        service.call(["status"], r => { if (r && r.ok && r.data) service.daemon = r.data.daemon === true; });
        loadRules();
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

    PageTitle { text: StanceI18n.t("Displays & modes") }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "bookmarks"
        title: StanceI18n.t("Saved modes")
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
                    selected: page.service.activeMode === modelData.name
                }
            }
        }
        Note {
            visible: page.service.modes.length === 0
            text: StanceI18n.t("No modes yet.")
        }
        Note { text: StanceI18n.t("Create, apply and edit modes in the panel's Modes card.") }
    }
    StanceSection {
        id: rulesCard
        visible: page.service.featureOn("rules")
        Layout.fillWidth: true
        theme: page.theme
        icon: "usb"
        title: StanceI18n.t("Hotplug rules")
        trailing: [
            StanceChip {
                theme: page.theme
                icon: page.addingRule ? "close" : "add"
                label: page.addingRule ? StanceI18n.t("Cancel") : StanceI18n.t("Add")
                enabled: page.rulesLoaded
                onClicked: page.addingRule = !page.addingRule
            }
        ]

        Dim {
            text: StanceI18n.t("When exactly these screens are connected, a mode is applied by the background service (`stance daemon`).")
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            font.pixelSize: page.theme.fontSize - 2
        }

        TextEdit {
            visible: !page.service.daemon
            readOnly: true
            selectByMouse: true
            text: StanceI18n.t("Background service is off — rules and returning re-plugged screens to their last place need it: systemctl --user enable --now stance.service")
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
                            text: "→ " + rrow.modelData.mode
                                + (rrow.modelData.on_power ? " · " + StanceI18n.f("on %1", StanceI18n.t(rrow.modelData.on_power === "ac" ? "charger" : "battery")) : "")
                                + (rrow.modelData.allow_extras ? " · " + StanceI18n.t("with night light, audio, brightness") : "")
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
            text: StanceI18n.t("No rules. Rules apply a mode on their own when screens are plugged in.")
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            font.pixelSize: page.theme.fontSize - 2
        }

        // add form
        ColumnLayout {
            visible: page.addingRule
            Layout.fillWidth: true
            spacing: 8
            Dim { text: StanceI18n.t("When these screens are connected") }
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
            Dim { text: StanceI18n.t("Apply this mode") }
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
            Dim { text: StanceI18n.t("Power source") }
            StanceSegmented {
                Layout.fillWidth: true
                theme: page.theme
                model: [{ label: StanceI18n.t("Any"), value: "" }, { label: StanceI18n.t("Charger"), value: "ac" }, { label: StanceI18n.t("Battery"), value: "battery" }]
                current: page.newPower
                onPicked: v => page.newPower = v
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                StanceSwitch { theme: page.theme; checked: page.newExtras; onToggled: v => page.newExtras = v }
                Body {
                    text: StanceI18n.t("Also set night light, audio and brightness")
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
                label: page.service.callBusy ? StanceI18n.t("Saving…") : StanceI18n.t("Add rule")
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
}
