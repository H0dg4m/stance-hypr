import QtQuick
import QtQuick.Layouts

// "Phone buttons": the buttons a paired phone shows, stored in deck.toml.
// Reads `deck buttons`, writes with `deck put` after every change (like the
// hotplug rules), `deck reset` goes back to the built-in set.
StanceSection {
    id: sec

    required property StanceService service
    icon: "dashboard_customize"
    title: StanceI18n.t("Phone buttons")

    property var buttons: []
    property bool custom: false
    property bool loaded: false
    property string error: ""
    property bool adding: false
    property bool resetAsk: false

    // ---- add-form state ---------------------------------------------------
    property string fLabel: ""
    property string fAction: "mode"
    property string fName: ""        // mode name
    property string fOp: "toggle"    // night: toggle | set
    property string fKelvin: "3500"
    property string fMonitor: ""
    property string fValue: "+10"
    property string fSink: ""
    property string fKind: "below"
    property string fRef: ""

    readonly property var actions: [
        { value: "mode", label: StanceI18n.t("Mode"), icon: "bookmarks" },
        { value: "night", label: StanceI18n.t("Night light"), icon: "bedtime" },
        { value: "brightness", label: StanceI18n.t("Brightness"), icon: "light_mode" },
        { value: "audio", label: StanceI18n.t("Audio output"), icon: "volume_up" },
        { value: "layout", label: StanceI18n.t("Screen layout"), icon: "monitor" },
        { value: "confirm", label: StanceI18n.t("Keep"), icon: "check" },
        { value: "revert", label: StanceI18n.t("Revert"), icon: "undo" }
    ]
    readonly property var kinds: ["left-of", "right-of", "above", "below", "mirror", "single"]
    readonly property bool kindNeedsRef: fKind !== "single"
    readonly property var monitorNames: service.monitors.map(m => m.name)
    readonly property var brightnessNames: service.quick && service.quick.brightness ? service.quick.brightness.map(b => b.monitor) : []
    readonly property var modeNames: service.modes.filter(m => !m.error).map(m => m.name)

    function iconFor(action) {
        const a = actions.find(x => x.value === action);
        return a ? a.icon : "touch_app";
    }

    function describe(b) {
        switch (b.action) {
        case "mode": return StanceI18n.f("Mode %1", b.name || "?");
        case "night": return StanceI18n.t("Night light") + " " + (b.op === "set" ? StanceI18n.f("at %1 K", b.kelvin) : StanceI18n.t(b.op || "toggle"))
            + (b.op === "on" && b.kelvin ? " " + StanceI18n.f("at %1 K", b.kelvin) : "");
        case "brightness": return StanceI18n.t("Brightness") + " " + (b.monitor || "?") + " " + (b.value || "");
        case "audio": return StanceI18n.t("Audio output") + " " + (b.sink || "?");
        case "layout": return StanceI18n.t("Layout") + ": " + (b.monitor || "?") + " " + (b.kind ? StanceI18n.t(b.kind) : "") + (b.reference ? " " + b.reference : "");
        case "confirm": return StanceI18n.t("Keep the pending display change");
        case "revert": return StanceI18n.t("Revert the pending display change");
        default: return b.action;
        }
    }

    function load() {
        error = "";
        service.deckButtons(r => {
            if (r && r.ok) {
                buttons = (r.data && r.data.button) || [];
                custom = r.data && r.data.custom === true;
                loaded = true;
            } else {
                error = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }

    function save(next, after) {
        error = "";
        service.deckPut(next, r => {
            if (r && r.ok) {
                // Re-read so the list shows exactly what the PC stored.
                load();
                if (after) after();
            } else {
                error = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }

    function move(i, d) {
        const j = i + d;
        if (j < 0 || j >= buttons.length) return;
        const n = buttons.slice();
        const t = n[i]; n[i] = n[j]; n[j] = t;
        save(n);
    }

    function remove(i) {
        save(buttons.filter((_, j) => j !== i));
    }

    function reset() {
        if (!resetAsk) {
            resetAsk = true;
            resetTimer.restart();
            return;
        }
        resetAsk = false;
        error = "";
        service.deckReset(r => {
            if (r && r.ok) load();
            else error = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
        });
    }

    function slug(label) {
        let s = label.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 28);
        if (s === "") s = "button";
        let id = s, n = 2;
        while (buttons.some(b => b.id === id)) id = s + "-" + (n++);
        return id;
    }

    function defaultLabel() {
        switch (fAction) {
        case "mode": return fName ? StanceI18n.f("Mode: %1", fName) : "";
        case "night": return fOp === "set" ? StanceI18n.f("Night %1 K", fKelvin) : StanceI18n.t("Night light");
        case "brightness": return fMonitor ? fMonitor + " " + fValue + " %" : "";
        case "audio": {
            const s = service.sinks.find(x => x.name === fSink);
            return s ? s.short : "";
        }
        case "layout": return fMonitor ? fMonitor + " " + fKind : "";
        case "confirm": return StanceI18n.t("Keep");
        case "revert": return StanceI18n.t("Revert");
        }
        return "";
    }

    function draftButton() {
        const label = (fLabel.trim() || defaultLabel()).slice(0, 40);
        const b = { id: slug(label), label: label, action: fAction, icon: iconFor(fAction) };
        switch (fAction) {
        case "mode": b.name = fName; break;
        case "night":
            b.op = fOp;
            if (fOp === "set") b.kelvin = parseInt(fKelvin);
            break;
        case "brightness": b.monitor = fMonitor; b.value = fValue.trim(); break;
        case "audio": b.sink = fSink; break;
        case "layout":
            b.kind = fKind; b.monitor = fMonitor;
            if (kindNeedsRef) b.reference = fRef;
            break;
        }
        return b;
    }

    // Why the form cannot be saved yet ("" when it can).
    readonly property string problem: {
        if (fLabel.trim() === "" && defaultLabel() === "") return StanceI18n.t("Pick what the button does.");
        switch (fAction) {
        case "mode": return fName === "" ? StanceI18n.t("Pick a mode.") : "";
        case "night":
            if (fOp === "set") {
                const k = parseInt(fKelvin);
                return isNaN(k) || k < 1000 || k > 10000 ? StanceI18n.t("Temperature: 1000–10000 K.") : "";
            }
            return "";
        case "brightness":
            if (fMonitor === "") return StanceI18n.t("Pick a screen.");
            return /^[+-]?[0-9]{1,3}$/.test(fValue.trim()) ? "" : StanceI18n.t("Value like 40, +10 or -10.");
        case "audio": return fSink === "" ? StanceI18n.t("Pick an output.") : "";
        case "layout":
            if (fMonitor === "") return StanceI18n.t("Pick the screen to move.");
            if (kindNeedsRef && (fRef === "" || fRef === fMonitor)) return StanceI18n.t("Pick a different reference screen.");
            return "";
        }
        return "";
    }

    function openForm() {
        fLabel = ""; fAction = "mode"; fName = ""; fOp = "toggle"; fKelvin = "3500";
        fMonitor = ""; fValue = "+10"; fSink = ""; fKind = "below"; fRef = "";
        adding = true;
    }

    function addButton() {
        if (problem !== "") return;
        save(buttons.concat([draftButton()]), () => adding = false);
    }

    Component.onCompleted: load()
    Timer { id: resetTimer; interval: 4000; onTriggered: sec.resetAsk = false }

    trailing: [
        StanceChip {
            theme: sec.theme
            icon: sec.adding ? "close" : "add"
            label: sec.adding ? StanceI18n.t("Cancel") : StanceI18n.t("Add")
            enabled: sec.loaded && !sec.service.callBusy
            onClicked: sec.adding ? sec.adding = false : sec.openForm()
        }
    ]

    component Dim: Text {
        textFormat: Text.PlainText
        color: sec.theme.textDim
        font.family: sec.theme.fontFamily
        font.pixelSize: sec.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    component Body: Text {
        textFormat: Text.PlainText
        color: sec.theme.text
        font.family: sec.theme.fontFamily
        font.pixelSize: sec.theme.fontSize
        elide: Text.ElideRight
    }

    // ---- the list ---------------------------------------------------------
    Dim {
        visible: !sec.loaded && sec.error === ""
        text: StanceI18n.t("Checking…")
    }
    RowLayout {
        visible: sec.loaded
        Layout.fillWidth: true
        spacing: 8
        Rectangle {
            implicitWidth: badge.implicitWidth + 14
            implicitHeight: 22
            radius: 11
            color: sec.custom ? sec.theme.accentSoft : sec.theme.surface
            Text {
                textFormat: Text.PlainText
                id: badge
                anchors.centerIn: parent
                text: sec.custom ? StanceI18n.t("custom set") : StanceI18n.t("default set")
                color: sec.custom ? sec.theme.text : sec.theme.textDim
                font.family: sec.theme.fontFamily
                font.pixelSize: sec.theme.fontSize - 3
            }
        }
        Dim {
            text: sec.custom ? StanceI18n.t("From deck.toml.") : StanceI18n.t("Built in. Adding a button saves your own set.")
            font.pixelSize: sec.theme.fontSize - 2
        }
    }

    Repeater {
        model: sec.buttons
        delegate: Rectangle {
            id: row
            required property var modelData
            required property int index
            Layout.fillWidth: true
            implicitHeight: rl.implicitHeight + 16
            radius: sec.theme.chipRadius
            color: sec.theme.surface
            RowLayout {
                id: rl
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8
                StanceIcon { theme: sec.theme; name: row.modelData.icon || sec.iconFor(row.modelData.action); size: sec.theme.fontSize + 5; color: sec.theme.textDim }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Body { text: row.modelData.label; Layout.fillWidth: true }
                    Dim { text: sec.describe(row.modelData); font.pixelSize: sec.theme.fontSize - 3; elide: Text.ElideRight }
                }
                StanceChip {
                    theme: sec.theme
                    icon: "arrow_upward"
                    enabled: row.index > 0 && !sec.service.callBusy
                    onClicked: sec.move(row.index, -1)
                }
                StanceChip {
                    theme: sec.theme
                    icon: "arrow_downward"
                    enabled: row.index < sec.buttons.length - 1 && !sec.service.callBusy
                    onClicked: sec.move(row.index, 1)
                }
                StanceChip {
                    theme: sec.theme
                    icon: "delete"
                    danger: true
                    enabled: !sec.service.callBusy
                    onClicked: sec.remove(row.index)
                }
            }
        }
    }
    Dim {
        visible: sec.loaded && sec.buttons.length === 0
        text: StanceI18n.t("No buttons: the phone shows an empty deck.")
    }

    // ---- add form -----------------------------------------------------------
    ColumnLayout {
        visible: sec.adding
        Layout.fillWidth: true
        spacing: 8

        Dim { text: StanceI18n.t("What it does") }
        Flow {
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.actions
                delegate: StanceChip {
                    required property var modelData
                    theme: sec.theme
                    icon: modelData.icon
                    label: modelData.label
                    selected: sec.fAction === modelData.value
                    onClicked: sec.fAction = modelData.value
                }
            }
        }

        // mode
        Dim { visible: sec.fAction === "mode"; text: StanceI18n.t("Mode") }
        Flow {
            visible: sec.fAction === "mode"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.modeNames
                delegate: StanceChip {
                    required property string modelData
                    theme: sec.theme
                    label: modelData
                    selected: sec.fName === modelData
                    onClicked: sec.fName = modelData
                }
            }
        }
        Dim { visible: sec.fAction === "mode" && sec.modeNames.length === 0; text: StanceI18n.t("No saved modes yet.") }

        // night
        Flow {
            visible: sec.fAction === "night"
            Layout.fillWidth: true
            spacing: 6
            StanceChip { theme: sec.theme; label: StanceI18n.t("Toggle"); selected: sec.fOp === "toggle"; onClicked: sec.fOp = "toggle" }
            StanceChip { theme: sec.theme; label: StanceI18n.t("Set temperature"); selected: sec.fOp === "set"; onClicked: sec.fOp = "set" }
        }
        RowLayout {
            visible: sec.fAction === "night" && sec.fOp === "set"
            Layout.fillWidth: true
            spacing: 8
            Dim { text: StanceI18n.t("Kelvin"); Layout.fillWidth: false }
            StanceField {
                theme: sec.theme
                Layout.preferredWidth: 100
                text: sec.fKelvin
                maxLength: 5
                validator: IntValidator { bottom: 0; top: 99999 }
                onEdited: v => sec.fKelvin = v
            }
        }

        // brightness
        Dim { visible: sec.fAction === "brightness"; text: StanceI18n.t("Screen") }
        Flow {
            visible: sec.fAction === "brightness"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.brightnessNames
                delegate: StanceChip {
                    required property string modelData
                    theme: sec.theme
                    icon: "monitor"
                    label: modelData
                    selected: sec.fMonitor === modelData
                    onClicked: sec.fMonitor = modelData
                }
            }
        }
        RowLayout {
            visible: sec.fAction === "brightness"
            Layout.fillWidth: true
            spacing: 8
            Dim { text: StanceI18n.t("Value"); Layout.fillWidth: false }
            StanceField {
                theme: sec.theme
                Layout.preferredWidth: 100
                text: sec.fValue
                maxLength: 4
                onEdited: v => sec.fValue = v
            }
            Dim { text: StanceI18n.t("40 sets, +10 / -10 steps"); font.pixelSize: sec.theme.fontSize - 3 }
        }

        // audio
        Dim { visible: sec.fAction === "audio"; text: StanceI18n.t("Output") }
        Flow {
            visible: sec.fAction === "audio"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.service.sinks
                delegate: StanceChip {
                    required property var modelData
                    theme: sec.theme
                    icon: modelData.bus === "usb" ? "headphones" : /hdmi/i.test(modelData.name) ? "tv" : "speaker"
                    label: modelData.short
                    maxWidth: 260
                    selected: sec.fSink === modelData.name
                    onClicked: sec.fSink = modelData.name
                }
            }
        }

        // layout
        Dim { visible: sec.fAction === "layout"; text: StanceI18n.t("Arrangement") }
        Flow {
            visible: sec.fAction === "layout"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.kinds
                delegate: StanceChip {
                    required property string modelData
                    theme: sec.theme
                    label: modelData
                    selected: sec.fKind === modelData
                    onClicked: sec.fKind = modelData
                }
            }
        }
        Dim { visible: sec.fAction === "layout"; text: sec.fKind === "single" ? "Only this screen stays on" : "Screen to move" }
        Flow {
            visible: sec.fAction === "layout"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.monitorNames
                delegate: StanceChip {
                    required property string modelData
                    theme: sec.theme
                    icon: "monitor"
                    label: modelData
                    selected: sec.fMonitor === modelData
                    onClicked: sec.fMonitor = modelData
                }
            }
        }
        Dim { visible: sec.fAction === "layout" && sec.kindNeedsRef; text: StanceI18n.t("Relative to") }
        Flow {
            visible: sec.fAction === "layout" && sec.kindNeedsRef
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: sec.monitorNames
                delegate: StanceChip {
                    required property string modelData
                    theme: sec.theme
                    icon: "monitor"
                    label: modelData
                    selected: sec.fRef === modelData
                    onClicked: sec.fRef = modelData
                }
            }
        }

        Dim { text: StanceI18n.t("Label on the phone (optional)") }
        StanceField {
            theme: sec.theme
            Layout.fillWidth: true
            placeholder: sec.defaultLabel()
            text: sec.fLabel
            maxLength: 40
            onEdited: v => sec.fLabel = v
            onAccepted: sec.addButton()
        }
        Text {
            textFormat: Text.PlainText
            visible: sec.problem !== ""
            text: sec.problem
            color: sec.theme.textDim
            font.family: sec.theme.fontFamily
            font.pixelSize: sec.theme.fontSize - 2
        }
        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            StanceChip {
                theme: sec.theme
                icon: "add"
                label: StanceI18n.t("Add button")
                selected: true
                enabled: sec.problem === "" && !sec.service.callBusy
                onClicked: sec.addButton()
            }
        }
    }

    // ---- footer -------------------------------------------------------------
    RowLayout {
        visible: sec.loaded && sec.custom
        Layout.fillWidth: true
        Item { Layout.fillWidth: true }
        StanceChip {
            theme: sec.theme
            icon: "restart_alt"
            label: sec.resetAsk ? StanceI18n.t("Sure? Click again") : StanceI18n.t("Reset to defaults")
            danger: true
            enabled: !sec.service.callBusy
            onClicked: sec.reset()
        }
    }
    Text {
        textFormat: Text.PlainText
        visible: sec.error !== ""
        text: sec.error
        color: sec.theme.danger
        font.family: sec.theme.fontFamily
        font.pixelSize: sec.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
