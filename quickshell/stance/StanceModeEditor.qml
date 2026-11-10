import QtQuick
import QtQuick.Layouts

// Mode editor page. Starts from `stance mode show` and changes only the
// fields it edits; everything else in the mode round-trips untouched.
ColumnLayout {
    id: ed
    required property StanceService service
    required property StanceTheme theme
    required property string modeName
    signal back

    spacing: 10

    property var draft: null
    property string orig: ""
    property string loadError: ""
    property string saveError: ""
    property bool askDiscard: false
    property bool askDelete: false
    property bool advanced: false
    readonly property bool dirty: draft !== null && JSON.stringify(draft) !== orig
    readonly property bool busy: service.callBusy
    readonly property var quick: service.quick
    readonly property var sinks: quick && quick.sinks ? quick.sinks.filter(s => !s.virtual || s.default) : []
    readonly property var displays: quick && quick.brightness ? quick.brightness : []

    // The panel's back button asks first when there are unsaved edits.
    function requestBack() {
        if (dirty)
            askDiscard = true;
        else
            back();
    }

    function load() {
        loadError = "";
        service.showMode(modeName, r => {
            if (r && r.ok) {
                draft = r.data;
                orig = JSON.stringify(draft);
            } else {
                loadError = r && r.error ? r.error.message : "stance returned no output";
            }
        });
    }

    function clone() { return JSON.parse(JSON.stringify(draft)); }

    function set(key, val) {
        const d = clone();
        if (val === undefined || val === null || val === "")
            delete d[key];
        else
            d[key] = val;
        draft = d;
    }

    function setIn(key, sub, val) {
        const d = clone();
        const o = d[key] && typeof d[key] === "object" ? d[key] : {};
        if (val === undefined)
            delete o[sub];
        else
            o[sub] = val;
        if (Object.keys(o).length === 0)
            delete d[key];
        else
            d[key] = o;
        draft = d;
    }

    function setMonitorEnabled(i, v) {
        const d = clone();
        d.monitor[i].enabled = v;
        draft = d;
    }

    function save() {
        saveError = "";
        service.putMode(modeName, draft, true, r => {
            if (r && r.ok)
                orig = JSON.stringify(draft);
            else
                saveError = r && r.error ? r.error.message : "stance returned no output";
        });
    }

    function remove() {
        saveError = "";
        service.removeMode(modeName, r => {
            if (r && r.ok)
                back();
            else
                saveError = r && r.error ? r.error.message : "stance returned no output";
        });
    }

    // Segmented value helpers: absent => "keep".
    function tri(v) { return v === undefined ? "keep" : v; }
    function nightValue() {
        const n = draft ? draft.night_light : undefined;
        return n === undefined ? "keep" : n === false ? "off" : n === true ? "on" : n;
    }
    function nightPicked(v) {
        set("night_light", v === "keep" ? undefined : v === "off" ? false : v === "on" ? true : v);
    }
    function nightCustom() {
        const n = nightValue();
        return typeof n === "number" && [3500, 4500, 5500].indexOf(n) < 0 ? n : 0;
    }
    function sinkSelected(s) {
        const a = draft ? draft.audio_sink : undefined;
        return a !== undefined && (a === s.name || a === String(s.id) || a === s.description);
    }
    function sinkKnown() {
        const a = draft ? draft.audio_sink : undefined;
        return a === undefined || sinks.some(s => sinkSelected(s));
    }
    function brightnessKeys() {
        const have = draft && draft.brightness ? Object.keys(draft.brightness) : [];
        const names = displays.map(d => d.monitor);
        return names.concat(have.filter(k => names.indexOf(k) < 0));
    }
    function hyprPicked(key, v) { setIn("hyprland", key, v === "keep" ? undefined : v); }
    function hyprValue(key) {
        const h = draft && draft.hyprland ? draft.hyprland[key] : undefined;
        return h === undefined ? "keep" : h;
    }
    function gapCustom(key) {
        const h = hyprValue(key);
        return h !== "keep" && [0, 5, 10, 20].indexOf(h) < 0;
    }
    function monitorSummary(m) {
        const p = [];
        if (m.mode) p.push(m.mode);
        if (m.position) p.push("at " + m.position);
        if (m.scale !== undefined) p.push("scale " + m.scale);
        if (m.mirror) p.push("mirror " + m.mirror);
        if (m.transform !== undefined) p.push("rotate " + m.transform);
        if (m.vrr !== undefined) p.push("vrr " + m.vrr);
        return p.join(" · ");
    }

    Component.onCompleted: load()

    component Dim: Text {
        color: ed.theme.textDim
        font.family: ed.theme.fontFamily
        font.pixelSize: ed.theme.fontSize - 1
    }
    component Body: Text {
        color: ed.theme.text
        font.family: ed.theme.fontFamily
        font.pixelSize: ed.theme.fontSize
    }
    component Err: Text {
        color: ed.theme.danger
        font.family: ed.theme.fontFamily
        font.pixelSize: ed.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    // ---- loading / error --------------------------------------------------
    StanceSection {
        visible: ed.draft === null
        Layout.fillWidth: true
        theme: ed.theme
        Dim { visible: ed.loadError === ""; text: "Loading…" }
        Err { visible: ed.loadError !== ""; text: ed.loadError }
        StanceChip {
            visible: ed.loadError !== ""
            theme: ed.theme
            icon: "refresh"
            label: "Retry"
            onClicked: ed.load()
        }
    }

    // ---- general ------------------------------------------------------------
    StanceSection {
        visible: ed.draft !== null
        Layout.fillWidth: true
        theme: ed.theme
        icon: "tune"
        title: "General"
        Dim { text: "Description" }
        StanceField {
            Layout.fillWidth: true
            theme: ed.theme
            placeholder: "What this mode is for"
            maxLength: 200
            text: ed.draft && ed.draft.description ? ed.draft.description : ""
            onEdited: v => ed.set("description", v)
        }
        Dim { text: "Power profile" }
        StanceSegmented {
            Layout.fillWidth: true
            theme: ed.theme
            model: [{ label: "Keep", value: "" }, { label: "Saver", value: "power-saver" }, { label: "Balanced", value: "balanced" }, { label: "Perf", value: "performance" }]
            current: ed.draft && ed.draft.power_profile ? ed.draft.power_profile : ""
            onPicked: v => ed.set("power_profile", v)
        }
    }

    // ---- quick settings -------------------------------------------------------
    StanceSection {
        visible: ed.draft !== null
        Layout.fillWidth: true
        theme: ed.theme
        icon: "bolt"
        title: "Quick settings"
        Dim { text: "Night light" }
        StanceSegmented {
            Layout.fillWidth: true
            theme: ed.theme
            model: [{ label: "Keep", value: "keep" }, { label: "Off", value: "off" }, { label: "On", value: "on" }, { label: "3.5k", value: 3500 }, { label: "4.5k", value: 4500 }, { label: "5.5k", value: 5500 }]
            current: ed.nightValue()
            onPicked: v => ed.nightPicked(v)
        }
        Dim { visible: ed.nightCustom() > 0; text: "Custom: " + ed.nightCustom() + " K (kept until you pick another)"; font.pixelSize: ed.theme.fontSize - 2 }

        Dim { text: "Audio output"; Layout.topMargin: 4 }
        Flow {
            Layout.fillWidth: true
            spacing: 6
            StanceChip {
                theme: ed.theme
                label: "Keep"
                selected: ed.draft && ed.draft.audio_sink === undefined
                onClicked: ed.set("audio_sink", undefined)
            }
            Repeater {
                model: ed.sinks
                delegate: StanceChip {
                    required property var modelData
                    theme: ed.theme
                    icon: modelData.virtual ? "graphic_eq" : modelData.bus === "usb" ? "headphones" : /hdmi/i.test(modelData.name) ? "tv" : "speaker"
                    label: modelData.short
                    maxWidth: 300
                    selected: ed.sinkSelected(modelData)
                    onClicked: ed.set("audio_sink", modelData.name)
                }
            }
            StanceChip {
                visible: !ed.sinkKnown()
                theme: ed.theme
                icon: "speaker"
                label: ed.draft && ed.draft.audio_sink ? ed.draft.audio_sink : ""
                maxWidth: 300
                selected: true
            }
        }

        Dim { text: "Brightness"; Layout.topMargin: 4; visible: ed.brightnessKeys().length > 0 }
        Repeater {
            model: ed.brightnessKeys()
            delegate: RowLayout {
                id: brow
                required property string modelData
                readonly property var cur: ed.draft && ed.draft.brightness ? ed.draft.brightness[modelData] : undefined
                Layout.fillWidth: true
                spacing: 8
                StanceSwitch {
                    theme: ed.theme
                    checked: brow.cur !== undefined
                    onToggled: v => ed.setIn("brightness", brow.modelData, v ? 60 : undefined)
                }
                Body { text: brow.modelData; Layout.preferredWidth: 64; elide: Text.ElideRight; font.pixelSize: ed.theme.fontSize - 1 }
                StanceSlider {
                    theme: ed.theme
                    Layout.fillWidth: true
                    enabled: brow.cur !== undefined
                    value: brow.cur !== undefined ? brow.cur : 0
                    onCommitted: v => ed.setIn("brightness", brow.modelData, Math.round(v))
                }
                Body {
                    text: brow.cur !== undefined ? brow.cur + "%" : "–"
                    horizontalAlignment: Text.AlignRight
                    Layout.preferredWidth: 36
                    font.pixelSize: ed.theme.fontSize - 1
                }
            }
        }
        Dim { text: "Saved settings apply with the mode; screens that are not connected are skipped."; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: ed.theme.fontSize - 2 }
    }

    // ---- screens (read mostly) ----------------------------------------------
    StanceSection {
        visible: ed.draft !== null && ed.draft.monitor !== undefined && ed.draft.monitor.length > 0
        Layout.fillWidth: true
        theme: ed.theme
        icon: "monitor"
        title: "Screens in this mode"
        Repeater {
            model: ed.draft && ed.draft.monitor ? ed.draft.monitor : []
            delegate: RowLayout {
                id: mrow
                required property var modelData
                required property int index
                Layout.fillWidth: true
                spacing: 10
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Body { text: mrow.modelData.match; elide: Text.ElideRight; Layout.fillWidth: true }
                    Dim { text: ed.monitorSummary(mrow.modelData); visible: text !== ""; elide: Text.ElideRight; Layout.fillWidth: true; font.pixelSize: ed.theme.fontSize - 2 }
                }
                StanceSwitch {
                    visible: mrow.modelData.enabled !== undefined
                    theme: ed.theme
                    checked: mrow.modelData.enabled === true
                    onToggled: v => ed.setMonitorEnabled(mrow.index, v)
                }
            }
        }
        Dim { text: "Positions and sizes come from the layout saved with the mode."; wrapMode: Text.Wrap; Layout.fillWidth: true; font.pixelSize: ed.theme.fontSize - 2 }
    }

    // ---- advanced -----------------------------------------------------------
    StanceSection {
        visible: ed.draft !== null
        Layout.fillWidth: true
        theme: ed.theme
        icon: "build"
        title: "Hyprland options"
        expanded: ed.advanced
        trailing: [
            StanceChip {
                theme: ed.theme
                icon: ed.advanced ? "expand_less" : "expand_more"
                label: ed.advanced ? "Hide" : "Show"
                onClicked: ed.advanced = !ed.advanced
            }
        ]
        ColumnLayout {
            visible: ed.advanced
            Layout.fillWidth: true
            spacing: 8
            Repeater {
                model: [{ key: "animations", label: "Animations" }, { key: "blur", label: "Blur" }, { key: "allow_tearing", label: "Allow tearing" }]
                delegate: ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4
                    Dim { text: modelData.label }
                    StanceSegmented {
                        Layout.fillWidth: true
                        theme: ed.theme
                        model: [{ label: "Keep", value: "keep" }, { label: "On", value: true }, { label: "Off", value: false }]
                        current: ed.hyprValue(modelData.key)
                        onPicked: v => ed.hyprPicked(modelData.key, v)
                    }
                }
            }
            Repeater {
                model: [{ key: "gaps_in", label: "Inner gaps" }, { key: "gaps_out", label: "Outer gaps" }]
                delegate: ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4
                    Dim { text: modelData.label + (ed.gapCustom(modelData.key) ? " · custom value kept" : "") }
                    StanceSegmented {
                        Layout.fillWidth: true
                        theme: ed.theme
                        model: [{ label: "Keep", value: "keep" }, { label: "0", value: 0 }, { label: "5", value: 5 }, { label: "10", value: 10 }, { label: "20", value: 20 }]
                        current: ed.hyprValue(modelData.key)
                        onPicked: v => ed.hyprPicked(modelData.key, v)
                    }
                }
            }
        }
        Dim {
            visible: ed.advanced && ed.draft && ed.draft.workspaces !== undefined
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            font.pixelSize: ed.theme.fontSize - 2
            text: "Workspaces: " + (ed.draft && ed.draft.workspaces ? Object.keys(ed.draft.workspaces).map(k => k + " → " + ed.draft.workspaces[k]).join(", ") : "") + " (edit in the mode file)"
        }
    }

    // ---- footer ---------------------------------------------------------------
    ColumnLayout {
        visible: ed.draft !== null
        Layout.fillWidth: true
        spacing: 8
        Err { visible: ed.saveError !== ""; text: ed.saveError }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            StanceChip {
                Layout.fillWidth: true
                Layout.preferredHeight: 40
                theme: ed.theme
                icon: "save"
                label: ed.busy ? "Saving…" : ed.dirty ? "Save" : "Saved"
                selected: ed.dirty
                enabled: ed.dirty && !ed.busy
                onClicked: ed.save()
            }
            StanceChip {
                Layout.preferredHeight: 40
                theme: ed.theme
                icon: "play_arrow"
                label: "Apply"
                tip: "Apply this mode now"
                enabled: !ed.dirty && !ed.busy && !ed.service.busy
                onClicked: ed.service.applyMode(ed.modeName)
            }
            StanceChip {
                Layout.preferredHeight: 40
                theme: ed.theme
                icon: "delete"
                danger: true
                tip: "Delete this mode"
                enabled: !ed.busy
                onClicked: ed.askDelete = true
            }
        }
        Rectangle {
            visible: ed.askDiscard
            Layout.fillWidth: true
            implicitHeight: dcol.implicitHeight + 16
            radius: ed.theme.chipRadius
            color: ed.theme.surface
            ColumnLayout {
                id: dcol
                anchors.fill: parent
                anchors.margins: 8
                spacing: 6
                Body { text: "Discard unsaved changes?" }
                RowLayout {
                    spacing: 8
                    StanceChip { theme: ed.theme; label: "Keep editing"; onClicked: ed.askDiscard = false }
                    StanceChip { theme: ed.theme; label: "Discard"; danger: true; selected: true; onClicked: { ed.askDiscard = false; ed.back(); } }
                }
            }
        }
        Rectangle {
            visible: ed.askDelete
            Layout.fillWidth: true
            implicitHeight: xcol.implicitHeight + 16
            radius: ed.theme.chipRadius
            color: ed.theme.surface
            ColumnLayout {
                id: xcol
                anchors.fill: parent
                anchors.margins: 8
                spacing: 6
                Body { text: "Delete mode “" + ed.modeName + "”?" }
                RowLayout {
                    spacing: 8
                    StanceChip { theme: ed.theme; label: "Cancel"; onClicked: ed.askDelete = false }
                    StanceChip { theme: ed.theme; label: "Delete"; danger: true; selected: true; enabled: !ed.busy; onClicked: { ed.askDelete = false; ed.remove(); } }
                }
            }
        }
    }
}
