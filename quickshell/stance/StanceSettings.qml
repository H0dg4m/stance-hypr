import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

// Panel settings page: what the panel shows, how long a display change waits
// for confirmation, hotplug rules and a short health summary.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    signal back
    // Category shown: general | appearance | displays | quick | clip | phone | features | health
    property string cat: "general"

    spacing: 10

    readonly property var prefs: service.prefs
    readonly property var toggles: [
        { key: "map", label: StanceI18n.t("Screen map"), icon: "grid_view" },
        { key: "screen", label: StanceI18n.t("Screen settings"), icon: "monitor" },
        { key: "modes", label: StanceI18n.t("Modes"), icon: "bookmarks" }
    ]
    readonly property var quickToggles: [
        { key: "night", label: StanceI18n.t("Night light"), icon: "bedtime" },
        { key: "brightness", label: StanceI18n.t("Brightness"), icon: "light_mode" },
        { key: "audio", label: StanceI18n.t("Audio output"), icon: "volume_up" }
    ]

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

    // ---- health state -------------------------------------------------------
    property string version: ""
    property var checks: []
    property bool showAllChecks: false
    readonly property var clipCheck: checks.find(c => c.id === "clip") || null
    readonly property bool cliphistRuns: clipCheck !== null && /cliphist/i.test(clipCheck.message)
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

    // ---- phone deck state -----------------------------------------------------
    property var deckInfo: null        // data of `deck devices`, null until loaded
    property var pair: null            // {payload, svg, fingerprint, until, baseline} while the QR shows
    property real pairNow: Date.now()
    property string pairError: ""
    property string unpairAsk: ""      // device waiting for the second click
    readonly property var deckServer: deckInfo ? deckInfo.server : null
    readonly property var deckList: deckServer ? deckServer.devices : (deckInfo ? deckInfo.devices : [])
    readonly property int pairLeft: pair ? Math.max(0, Math.ceil((pair.until - pairNow) / 1000)) : 0
    readonly property var deckCheck: checks.find(c => c.id === "deck" && c.status === "warn")

    function loadDeck(after) {
        service.deckDevices(r => {
            if (r && r.ok)
                deckInfo = r.data;
            if (after)
                after();
        });
    }
    function hidePair() {
        pair = null;
        pairError = "";
    }
    function startPair() {
        pairError = "";
        service.deckPair(r => {
            if (r && r.ok) {
                const n = deckList.length;
                pairNow = Date.now();
                pair = { payload: r.data.payload, svg: r.data.svg, fingerprint: r.data.fingerprint, until: Date.now() + r.data.expires_ms, baseline: n };
            } else {
                pairError = r && r.error ? (r.error.code === "deck_not_running" ? "The deck server is not running." : r.error.message) : StanceI18n.t("stance returned no output");
            }
        });
    }
    function unpair(name) {
        if (unpairAsk !== name) {
            unpairAsk = name;
            return;
        }
        unpairAsk = "";
        service.deckUnpair(name, r => {
            if (r && !r.ok)
                pairError = r.error ? r.error.message : StanceI18n.t("unpair failed");
            loadDeck();
        });
    }
    // Countdown, and polling for a new device while the QR shows.
    Timer {
        interval: 1000; repeat: true; running: page.pair !== null
        onTriggered: {
            page.pairNow = Date.now();
            if (page.pairLeft <= 0)
                page.hidePair();
        }
    }
    Timer {
        interval: 2000; repeat: true; running: page.pair !== null
        onTriggered: page.loadDeck(() => {
            if (page.pair && page.deckList.length > page.pair.baseline)
                page.hidePair();
        })
    }

    Component.onCompleted: {
        loadDeck();
        service.call(["status"], r => { if (r && r.ok && r.data) service.daemon = r.data.daemon === true; });
        loadRules();
        loadChecks();
        loadClip();
        verProc.running = true;
    }

    Process {
        id: verProc
        command: [page.service.bin, "--version"]
        stdout: StdioCollector { onStreamFinished: page.version = text.trim() }
    }

    // ---- clipboard shelf config (stance clip config) ---------------------------
    property var clipCfg: null
    property string clipError: ""
    readonly property var clipRetention: [1, 3, 7, 15, 30]

    // Capture pause (stance clip pause/resume): unix seconds, 0 = not paused.
    property real clipNow: Date.now()
    readonly property real clipPausedUntil: clipCfg && clipCfg.paused_until ? clipCfg.paused_until : 0
    readonly property bool clipPaused: clipPausedUntil * 1000 > clipNow
    Timer {
        interval: 15000; repeat: true; running: page.clipPausedUntil > 0
        onTriggered: page.clipNow = Date.now()
    }
    // arg: "15m" | "1h" | "24h" | "resume"
    function pauseClip(arg) {
        service.call(arg === "resume" ? ["clip", "resume"] : ["clip", "pause", arg], r => {
            if (r && r.ok) {
                clipNow = Date.now();
                loadClip();
            } else {
                clipError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }
    function resumeClip() { pauseClip("resume"); }
    // "HH:mm", with the weekday when it is not today.
    function pauseText(secs) {
        const d = new Date(secs * 1000);
        const same = d.toDateString() === new Date().toDateString();
        return same ? Qt.formatTime(d, "HH:mm") : Qt.formatDateTime(d, "ddd HH:mm");
    }

    function loadClip() {
        service.call(["clip", "config", "show"], r => {
            if (r && r.ok && r.data) {
                clipCfg = r.data;
                clipNow = Date.now();
                clipError = "";
            } else {
                clipError = r && r.error ? r.error.message : StanceI18n.t("this stance has no clip support");
            }
        });
    }

    // key: "enabled" | "text" | "image" | "retention_days"
    function setClip(key, value) {
        const next = {
            enabled: clipCfg.enabled === true,
            text: clipCfg.text !== false,
            image: clipCfg.image !== false,
            retention_days: clipCfg.retention_days || 30
        };
        next[key] = value;
        service.call(["clip", "config", "put", "--data", JSON.stringify(next)], r => {
            if (r && r.ok) {
                clipCfg = r.data && r.data.enabled !== undefined ? r.data : Object.assign({}, clipCfg, next);
                clipError = "";
            } else {
                clipError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
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
    // One "Show in panel" switch per entry of `keys`.
    component ShowList: ColumnLayout {
        id: sl
        property var keys: []
        Layout.fillWidth: true
        spacing: page.theme.spacing + 2
        Repeater {
            model: sl.keys
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
    // Pause time choices (15 min / 1 h / until resumed = 24 h).
    component PauseRow: Flow {
        spacing: 6
        Layout.fillWidth: true
        StanceChip { theme: page.theme; icon: "pause"; label: StanceI18n.t("15 min"); onClicked: page.pauseClip("15m") }
        StanceChip { theme: page.theme; icon: "pause"; label: StanceI18n.t("1 hour"); onClicked: page.pauseClip("1h") }
        StanceChip { theme: page.theme; icon: "pause"; label: StanceI18n.t("Until resumed"); onClicked: page.pauseClip("24h") }
    }
    component Body: Text {
        textFormat: Text.PlainText
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize
    }

    // ---- General ----------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "general"
        Layout.fillWidth: true
        spacing: 10
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
            icon: "visibility"
            title: StanceI18n.t("Show in panel")
            ShowList { keys: page.toggles }
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

    // ---- Appearance ---------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "appearance"
        Layout.fillWidth: true
        spacing: 10
        PageTitle { text: StanceI18n.t("Appearance") }
        StanceAppearance {
            Layout.fillWidth: true
            service: page.service
            theme: page.theme
        }
    }

    // ---- Displays & modes ----------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "displays"
        Layout.fillWidth: true
        spacing: 10
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

    // ---- Quick settings ------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "quick"
        Layout.fillWidth: true
        spacing: 10
        PageTitle { text: StanceI18n.t("Quick settings") }
        StanceSection {
            Layout.fillWidth: true
            theme: page.theme
            icon: "visibility"
            title: StanceI18n.t("Show in panel")
            ShowList { keys: page.quickToggles }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                StanceIcon { theme: page.theme; name: "density_small"; size: page.theme.fontSize + 3; color: page.theme.textDim }
                Body { text: StanceI18n.t("Compact quick controls"); Layout.fillWidth: true }
                StanceSwitch {
                    theme: page.theme
                    checked: page.prefs.compact === true
                    onToggled: v => page.service.setPref("compact", v)
                }
            }
        }
    }

    // ---- Clipboard ----------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "clip"
        Layout.fillWidth: true
        spacing: 10
        PageTitle { text: StanceI18n.t("Clipboard") }
        Rectangle {
            visible: page.cliphistRuns
            Layout.fillWidth: true
            implicitHeight: warnRow.implicitHeight + 24
            radius: page.theme.radius
            color: page.theme.card
            border.width: 1
            border.color: page.theme.danger
            RowLayout {
                id: warnRow
                x: 12
                y: 12
                width: parent.width - 24
                spacing: 10
                StanceIcon { theme: page.theme; name: "warning"; fill: true; color: page.theme.danger; Layout.alignment: Qt.AlignTop }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Body { text: StanceI18n.t("cliphist is also running"); font.weight: Font.DemiBold; Layout.fillWidth: true }
                    Note {
                        text: StanceI18n.t("cliphist also runs and keeps everything, including copies stance skips. Turn it off in your shell if you want stance to be the only clipboard history.")
                        color: page.theme.text
                    }
                }
            }
        }
        StanceSection {
            Layout.fillWidth: true
            theme: page.theme
            icon: "content_paste"
            visible: page.service.featureOn("clip")
            title: StanceI18n.t("History")
            Dim {
                visible: page.clipCfg === null
                text: page.clipError !== "" ? page.clipError : StanceI18n.t("Loading…")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            ColumnLayout {
                visible: page.clipCfg !== null
                Layout.fillWidth: true
                spacing: 8
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Body { text: StanceI18n.t("Keep a clipboard history"); Layout.fillWidth: true }
                    StanceSwitch {
                        theme: page.theme
                        checked: page.clipCfg !== null && page.clipCfg.enabled === true
                        onToggled: v => page.setClip("enabled", v)
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Body { text: StanceI18n.t("Capture text and links"); Layout.fillWidth: true }
                    StanceSwitch {
                        theme: page.theme
                        checked: page.clipCfg !== null && page.clipCfg.text !== false
                        onToggled: v => page.setClip("text", v)
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Body { text: StanceI18n.t("Capture images"); Layout.fillWidth: true }
                    StanceSwitch {
                        theme: page.theme
                        checked: page.clipCfg !== null && page.clipCfg.image !== false
                        onToggled: v => page.setClip("image", v)
                    }
                }
                Body { text: StanceI18n.t("Keep unpinned items for"); Layout.topMargin: 4 }
                StanceSegmented {
                    Layout.fillWidth: true
                    theme: page.theme
                    model: page.clipRetention.map(d => ({ label: StanceI18n.f(d === 1 ? "%1 day" : "%1 days", d), value: d }))
                    current: page.clipCfg !== null ? page.clipCfg.retention_days : null
                    onPicked: v => page.setClip("retention_days", v)
                }
                Body { text: StanceI18n.t("Capture"); Layout.topMargin: 4 }
                RowLayout {
                    visible: page.clipPaused
                    Layout.fillWidth: true
                    spacing: 8
                    StanceIcon { theme: page.theme; name: "pause_circle"; fill: true; color: page.theme.accent }
                    Body {
                        text: StanceI18n.f("Paused until %1", page.pauseText(page.clipPausedUntil))
                        color: page.theme.accent
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                    }
                    StanceChip { theme: page.theme; icon: "play_arrow"; label: StanceI18n.t("Resume"); selected: true; onClicked: page.resumeClip() }
                }
                Note { visible: !page.clipPaused; text: StanceI18n.t("Pause capture, for example while you copy something private.") }
                PauseRow { visible: !page.clipPaused }
                Note {
                    text: StanceI18n.t("History is stored unencrypted in your data directory. Copies a password manager marks as sensitive are never kept. Needs the stance-clip units running.")
                }
                Dim {
                    visible: page.clipError !== ""
                    text: page.clipError
                    color: page.theme.danger
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
        }


    }

    // ---- Phone ----------------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "phone"
        Layout.fillWidth: true
        spacing: 10
        PageTitle { text: StanceI18n.t("Phone") }
        StanceSection {
            Layout.fillWidth: true
            theme: page.theme
            icon: "smartphone"
            visible: page.service.featureOn("deck")
            title: StanceI18n.t("Phone")
            trailing: [
                StanceChip {
                    theme: page.theme
                    icon: page.pair ? "close" : "qr_code_2"
                    label: page.pair ? StanceI18n.t("Hide") : StanceI18n.t("Pair a phone")
                    enabled: page.deckServer !== null && !page.service.callBusy
                    onClicked: page.pair ? page.hidePair() : page.startPair()
                }
            ]

            Dim {
                visible: page.deckInfo === null
                text: StanceI18n.t("Checking…")
            }
            Dim {
                visible: page.deckServer !== null
                text: page.deckServer ? StanceI18n.f(page.deckServer.loopback ? "Running on port %1 (this PC only)" : "Running on port %1", page.deckServer.port) + (page.deckServer.hosts.length ? " · " + page.deckServer.hosts.join(", ") : "") : ""
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                font.pixelSize: page.theme.fontSize - 2
            }
            TextEdit {
                visible: page.deckInfo !== null && page.deckServer === null
                readOnly: true
                selectByMouse: true
                text: StanceI18n.t("Phone deck is off. Start it: systemctl --user enable --now stance-deck.service (or run `stance deck serve`)")
                wrapMode: TextEdit.Wrap
                Layout.fillWidth: true
                color: page.theme.textDim
                selectionColor: page.theme.accent
                selectedTextColor: page.theme.accentText
                font.family: page.theme.fontFamily
                font.pixelSize: page.theme.fontSize - 2
            }
            RowLayout {
                visible: page.deckCheck !== undefined && page.deckCheck !== null
                Layout.fillWidth: true
                spacing: 8
                StanceIcon { theme: page.theme; name: "warning"; size: page.theme.fontSize + 3; color: page.theme.textDim; Layout.alignment: Qt.AlignTop }
                TextEdit {
                    readOnly: true
                    selectByMouse: true
                    text: page.deckCheck ? page.deckCheck.message : ""
                    wrapMode: TextEdit.Wrap
                    Layout.fillWidth: true
                    color: page.theme.textDim
                    selectionColor: page.theme.accent
                    selectedTextColor: page.theme.accentText
                    font.family: page.theme.fontFamily
                    font.pixelSize: page.theme.fontSize - 2
                }
            }

            // QR while the pairing window is open
            ColumnLayout {
                visible: page.pair !== null
                Layout.fillWidth: true
                spacing: 6
                Rectangle {
                    Layout.alignment: Qt.AlignHCenter
                    implicitWidth: 236
                    implicitHeight: 236
                    radius: 8
                    color: "white"
                    Image {
                        anchors.centerIn: parent
                        width: 220
                        height: 220
                        source: page.pair ? "file://" + page.pair.svg.split("/").map(encodeURIComponent).join("/") : ""
                        cache: false
                        sourceSize: Qt.size(220, 220)
                        fillMode: Image.PreserveAspectFit
                    }
                }
                Body {
                    Layout.alignment: Qt.AlignHCenter
                    text: StanceI18n.f("Scan within %1 s", page.pairLeft)
                }
                Dim {
                    Layout.alignment: Qt.AlignHCenter
                    text: page.pair ? StanceI18n.f("This PC's key: %1", page.pair.fingerprint) : ""
                    font.pixelSize: page.theme.fontSize - 2
                }
                TextEdit {
                    readOnly: true
                    selectByMouse: true
                    text: page.pair ? page.pair.payload : ""
                    wrapMode: TextEdit.WrapAnywhere
                    Layout.fillWidth: true
                    color: page.theme.textDim
                    selectionColor: page.theme.accent
                    selectedTextColor: page.theme.accentText
                    font.family: "monospace"
                    font.pixelSize: page.theme.fontSize - 5
                }
                // The panel closes when focus moves to a terminal, so typing the
                // code by hand is not an option: copy a ready command instead.
                // (Single use and 120 s, so a clipboard history copy is harmless.)
                StanceChip {
                    Layout.alignment: Qt.AlignHCenter
                    theme: page.theme
                    icon: copied ? "check" : "content_copy"
                    label: copied ? StanceI18n.t("Copied — paste in a terminal") : StanceI18n.t("Copy test command")
                    property bool copied: false
                    onClicked: {
                        Quickshell.clipboardText = "stance deck test pair '" + page.pair.payload + "'";
                        copied = true;
                    }
                }
                Dim {
                    textFormat: Text.PlainText
                    text: StanceI18n.t("Or give this code to `stance deck test pair`. Anyone who sees it now can pair, so it disappears after use.")
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                    font.pixelSize: page.theme.fontSize - 2
                }
            }

            Repeater {
                model: page.deckList
                delegate: Rectangle {
                    id: drow
                    required property var modelData
                    Layout.fillWidth: true
                    implicitHeight: dr.implicitHeight + 16
                    radius: page.theme.chipRadius
                    color: page.theme.surface
                    RowLayout {
                        id: dr
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 8
                        Rectangle {
                            implicitWidth: 8
                            implicitHeight: 8
                            radius: 4
                            visible: drow.modelData.connected !== undefined
                            color: drow.modelData.connected ? page.theme.accent : "transparent"
                            border.width: 1
                            border.color: drow.modelData.connected ? page.theme.accent : page.theme.textDim
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Body { text: drow.modelData.name; elide: Text.ElideRight; Layout.fillWidth: true }
                            Dim {
                                text: drow.modelData.connected === true ? StanceI18n.t("connected") : StanceI18n.t("not connected")
                                visible: drow.modelData.connected !== undefined
                                font.pixelSize: page.theme.fontSize - 3
                            }
                        }
                        StanceChip {
                            theme: page.theme
                            icon: "link_off"
                            label: page.unpairAsk === drow.modelData.name ? StanceI18n.t("Sure?") : StanceI18n.t("Unpair")
                            danger: true
                            enabled: !page.service.callBusy
                            onClicked: page.unpair(drow.modelData.name)
                        }
                    }
                }
            }
            Dim {
                visible: page.deckInfo !== null && page.deckList.length === 0
                text: StanceI18n.t("No phones paired.")
                font.pixelSize: page.theme.fontSize - 2
            }
            Text {
                visible: page.pairError !== ""
                text: page.pairError
                color: page.theme.danger
                font.family: page.theme.fontFamily
                font.pixelSize: page.theme.fontSize - 1
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }

        StanceDeckButtons {
            visible: page.service.featureOn("deck")
            Layout.fillWidth: true
            theme: page.theme
            service: page.service
        }


    }

    // ---- Features ------------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "features"
        Layout.fillWidth: true
        spacing: 10
        PageTitle { text: StanceI18n.t("Features") }
        StanceSection {
            Layout.fillWidth: true
            theme: page.theme
            icon: "extension"
            Repeater {
                model: [
                    { name: "clip", label: "Clipboard shelf", hint: "History and the shelf", icon: "content_paste" },
                    { name: "deck", label: "Phone deck", hint: "Control the PC from a phone", icon: "smartphone" },
                    { name: "rules", label: "Hotplug rules", hint: "Apply modes when screens are plugged in", icon: "usb" },
                    { name: "quick", label: "Quick controls", hint: "Night light, brightness and audio in the panel", icon: "tune" }
                ]
                delegate: RowLayout {
                    id: frow2
                    required property var modelData
                    readonly property var info: page.service.featureList.find(f => f.name === modelData.name) || null
                    Layout.fillWidth: true
                    spacing: 10
                    StanceIcon { theme: page.theme; name: frow2.modelData.icon; size: page.theme.fontSize + 3; color: page.theme.textDim; Layout.alignment: Qt.AlignTop }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Body { text: StanceI18n.t(frow2.modelData.label); Layout.fillWidth: true }
                        Dim {
                            text: StanceI18n.t(frow2.modelData.hint)
                            font.pixelSize: page.theme.fontSize - 3
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                        Dim {
                            visible: page.service.featureOn(frow2.modelData.name) && frow2.info !== null && frow2.info.unit_active === false
                            text: StanceI18n.t("Its background service is not running.")
                            color: page.theme.danger
                            font.pixelSize: page.theme.fontSize - 3
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                    }
                    StanceSwitch {
                        theme: page.theme
                        checked: page.service.featureOn(frow2.modelData.name)
                        onToggled: v => page.service.setFeature(frow2.modelData.name, v)
                    }
                }
            }
            Dim {
                visible: page.service.featureError !== ""
                text: page.service.featureError
                color: page.theme.danger
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Note { text: StanceI18n.t("Turning a feature off also stops its background service; nothing stored is deleted.") }
        }
    }

    // ---- Health & about --------------------------------------------------------------
    ColumnLayout {
        visible: page.cat === "health"
        Layout.fillWidth: true
        spacing: 10
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
}
