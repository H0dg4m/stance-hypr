import QtQuick
import Qt.labs.folderlistmodel
import Quickshell
import Quickshell.Io

// Talks to the `stance` CLI (JSON only). Idle cost is zero: monitors are
// read when a panel opens, and the pending change is noticed through an
// inotify watch on the state directory, not by polling.
Item {
    id: svc

    property string bin: "stance"
    property var monitors: []
    property var modes: []
    property string activeMode: ""
    property string lastError: ""
    property string lastNote: ""
    property bool busy: actProc.running
    // `stance quick` result: { night, brightness, sinks, errors } or null.
    property var quick: null
    property string quickError: ""
    property bool quickBusy: quickActProc.running
    // A refresh asked for while one runs, and one write queued behind the
    // running one (two sliders, or a wheel commit during a night click).
    property bool quickDirty: false
    property var quickQueued: null

    // { label, deadline_ms, generation } or null
    property var pending: null
    // The hotplug daemon (`stance daemon`) is running.
    property bool daemon: false
    property real now: Date.now()
    readonly property real remainingMs: pending ? Math.max(0, pending.deadline_ms - now) : 0
    readonly property real timeoutMs: pending ? Math.max(1, pending.timeout_ms || 12000) : 1

    readonly property string stateDir: (Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")) + "/stance"
    readonly property string session: Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE") || ""

    // ---- panel preferences (UI only, ~/.config/stance/panel.json) -----------
    readonly property string configDir: (Quickshell.env("XDG_CONFIG_HOME") || (Quickshell.env("HOME") + "/.config")) + "/stance"
    readonly property var prefDefaults: ({
        show: { map: true, screen: true, night: true, brightness: true, audio: true, modes: true },
        confirmTimeout: 12,
        compact: false,
        theme: "",
        themeOverrides: {},
        lang: "auto",
        settingsPage: "general"
    })
    readonly property var settingsPages: ["general", "appearance", "displays", "quick", "clip", "phone", "features", "health"]
    property var prefs: prefDefaults
    readonly property int confirmTimeout: prefs.confirmTimeout

    // ---- theme (docs/themes.md) ------------------------------------------------
    // The StanceTheme this service drives; the embedding shell sets it.
    property var themeTarget: null
    // Used when panel.json has no `theme`: "stance-dark" standalone, "shell"
    // for integrations that pass their own colours through StanceTheme.shell.
    property string defaultTheme: "stance-dark"
    // `stance theme list --json` entries: { builtin, theme: { id, name, dark, colors, metrics } }
    property var themes: []
    property string themeError: ""
    readonly property string activeThemeId: prefs.theme !== "" ? prefs.theme : defaultTheme
    readonly property var activeThemeEntry: themes.find(e => e.theme.id === activeThemeId) || null
    readonly property var colorKeys: ["background", "card", "surface", "surfaceHover", "border", "text", "textDim", "accent", "accentText", "danger", "dangerText", "scrim"]
    readonly property var metricSpec: ({
        radius: { min: 0, max: 40 }, chipRadius: { min: 0, max: 30 }, fontSize: { min: 8, max: 32 },
        animMs: { min: 0, max: 1000 }, padding: { min: 0, max: 40 }, spacing: { min: 0, max: 32 },
        opacity: { min: 0.5, max: 1 }
    })

    function isHex(v) { return typeof v === "string" && /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(v); }

    // A valid override value, or undefined.
    function normOverride(key, v) {
        if (colorKeys.indexOf(key) >= 0)
            return isHex(v) ? v.toLowerCase() : undefined;
        if (key === "fontFamily" || key === "iconFont")
            return typeof v === "string" && v.length <= 64 ? v : undefined;
        const m = metricSpec[key];
        if (m && typeof v === "number" && isFinite(v))
            return Math.max(m.min, Math.min(m.max, key === "opacity" ? Math.round(v * 100) / 100 : Math.round(v)));
        return undefined;
    }

    // Unknown keys, wrong types and out-of-range values fall back to defaults.
    function normPrefs(o) {
        const d = prefDefaults;
        const out = { show: {}, confirmTimeout: d.confirmTimeout, compact: d.compact, theme: "", themeOverrides: {}, lang: "auto", settingsPage: "general" };
        const show = o && typeof o.show === "object" && o.show ? o.show : {};
        for (const k of Object.keys(d.show))
            out.show[k] = typeof show[k] === "boolean" ? show[k] : d.show[k];
        if (o && typeof o.confirmTimeout === "number" && isFinite(o.confirmTimeout))
            out.confirmTimeout = Math.max(5, Math.min(60, Math.round(o.confirmTimeout)));
        if (o && typeof o.compact === "boolean")
            out.compact = o.compact;
        if (o && typeof o.theme === "string" && /^[a-z0-9-]{1,40}$/.test(o.theme))
            out.theme = o.theme;
        if (o && typeof o.themeOverrides === "object" && o.themeOverrides) {
            for (const k of Object.keys(o.themeOverrides)) {
                const v = normOverride(k, o.themeOverrides[k]);
                if (v !== undefined)
                    out.themeOverrides[k] = v;
            }
        }
        if (o && (o.lang === "en" || o.lang === "tr" || o.lang === "auto"))
            out.lang = o.lang;
        if (o && settingsPages.indexOf(o.settingsPage) >= 0)
            out.settingsPage = o.settingsPage;
        return out;
    }

    // key: "show.<name>" | "confirmTimeout" | "compact" | "theme" | "lang" | "settingsPage"
    function setPref(key, value) {
        const p = JSON.parse(JSON.stringify(prefs));
        if (key.startsWith("show."))
            p.show[key.slice(5)] = value;
        else
            p[key] = value;
        prefs = normPrefs(p);
        prefsSave.restart();
    }

    function resetPrefs() {
        // Keep the open settings category.
        prefs = normPrefs({ settingsPage: prefs.settingsPage });
        prefsSave.restart();
    }

    // Picking a theme starts from that theme: tweaks are dropped.
    function pickTheme(id) {
        const p = JSON.parse(JSON.stringify(prefs));
        p.theme = id === defaultTheme ? "" : id;
        p.themeOverrides = {};
        prefs = normPrefs(p);
        prefsSave.restart();
    }
    // value undefined / "" removes the tweak. Returns false when it is invalid.
    function setOverride(key, value) {
        const p = JSON.parse(JSON.stringify(prefs));
        if (value === undefined || value === "" && key !== "fontFamily" && key !== "iconFont") {
            delete p.themeOverrides[key];
        } else {
            if (normOverride(key, value) === undefined)
                return false;
            p.themeOverrides[key] = value;
        }
        prefs = normPrefs(p);
        prefsSave.restart();
        return true;
    }
    function resetOverrides() {
        const p = JSON.parse(JSON.stringify(prefs));
        p.themeOverrides = {};
        prefs = normPrefs(p);
        prefsSave.restart();
    }

    onPrefsChanged: applyTheme()
    onThemesChanged: applyTheme()
    onThemeTargetChanged: applyTheme()
    onDefaultThemeChanged: applyTheme()

    function applyTheme() {
        StanceI18n.lang = prefs.lang;
        if (!themeTarget)
            return;
        // Not through activeThemeId / activeThemeEntry: those bindings may not
        // have been re-evaluated yet when a change handler calls this.
        const id = prefs.theme !== "" ? prefs.theme : defaultTheme;
        let obj = null;
        if (id !== "shell") {
            const e = themes.find(x => x.theme.id === id) || themes.find(x => x.theme.id === "stance-dark");
            obj = e ? e.theme : null;
        }
        themeTarget.apply(obj, prefs.themeOverrides);
    }

    function loadThemes() {
        call(["theme", "list"], r => {
            if (r && r.ok && r.data && r.data.themes) {
                themes = r.data.themes.filter(e => e && e.theme && typeof e.theme.id === "string");
                themeError = "";
            } else {
                themeError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }
    // done({ok, error})
    function themeCall(args, done) {
        call(args, r => {
            if (r && r.ok)
                loadThemes();
            if (done)
                done(r && r.ok ? { ok: true, data: r.data } : { ok: false, error: r && r.error ? r.error.message : StanceI18n.t("stance returned no output"), code: r && r.error ? r.error.code : "" });
        });
    }
    function putTheme(obj, force, done) {
        themeCall(["theme", "put", "--data", JSON.stringify(obj)].concat(force ? ["--force"] : []), done);
    }
    function importTheme(path, force, done) {
        themeCall(["theme", "import", path].concat(force ? ["--force"] : []), done);
    }
    function exportTheme(id, path, force, done) {
        themeCall(["theme", "export", id, "--out", path].concat(force ? ["--force"] : []), done);
    }
    function removeTheme(id, done) {
        themeCall(["theme", "rm", id], r => {
            // The removed theme was the chosen one: fall back to the default.
            if (r.ok && prefs.theme === id)
                pickTheme(defaultTheme);
            if (done)
                done(r);
        });
    }

    // ---- features (stance feature) -------------------------------------------
    // name -> bool; a missing name counts as on.
    property var features: ({})
    property var featureList: []
    property string featureError: ""
    function featureOn(name) { return features[name] !== false; }
    function loadFeatures() {
        call(["feature", "list"], r => {
            if (r && r.ok && r.data && r.data.features) {
                const m = {};
                for (const f of r.data.features)
                    m[f.name] = f.on !== false;
                features = m;
                featureList = r.data.features;
                featureError = "";
            } else {
                featureError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }
    function setFeature(name, on, done) {
        // Optimistic: the section hides / shows right away.
        const m = Object.assign({}, features);
        m[name] = on;
        features = m;
        call(["feature", on ? "on" : "off", name], r => {
            if (!(r && r.ok))
                featureError = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            loadFeatures();
            if (done)
                done(r);
        });
    }

    Component.onCompleted: {
        loadThemes();
        loadFeatures();
    }

    Process {
        id: mkdirProc
        command: ["/usr/bin/mkdir", "-p", svc.configDir]
        running: true
        onExited: prefsFile.reload()
    }

    Timer {
        id: prefsSave
        interval: 500
        onTriggered: {
            // Defaults stay out of the file.
            const p = JSON.parse(JSON.stringify(svc.prefs));
            if (p.theme === "") delete p.theme;
            if (Object.keys(p.themeOverrides).length === 0) delete p.themeOverrides;
            if (p.lang === "auto") delete p.lang;
            if (p.settingsPage === "general") delete p.settingsPage;
            prefsFile.setText(JSON.stringify(p, null, 1) + "\n");
        }
    }

    FileView {
        id: prefsFile
        path: svc.configDir + "/panel.json"
        printErrors: false
        onLoaded: svc.prefs = svc.normPrefs(svc.parse(text()))
    }

    // ---- generic CLI calls with a callback (mode editor, rules, doctor) ------
    property int callsRunning: 0
    readonly property bool callBusy: callsRunning > 0

    // `done(result)` gets the parsed {ok,data|error} object, or null when
    // stance printed nothing usable.
    function call(args, done) {
        callsRunning++;
        const p = callComp.createObject(svc, { command: [svc.bin].concat(args).concat(["--json"]), done: done });
        p.running = true;
        return p;
    }

    function showMode(name, done) { call(["mode", "show", name], done); }
    function putMode(name, data, force, done) {
        call(["mode", "put", name, "--data", JSON.stringify(data)].concat(force ? ["--force"] : []), r => {
            modesProc.running = true;
            if (done) done(r);
        });
    }
    function saveMode(name, withQuick, done) {
        call(["mode", "save", name].concat(withQuick ? ["--with-quick"] : []), r => {
            modesProc.running = true;
            if (done) done(r);
        });
    }
    function removeMode(name, done) {
        call(["mode", "rm", name], r => {
            modesProc.running = true;
            if (done) done(r);
        });
    }
    function getRules(done) { call(["rules"], done); }
    function putRules(data, done) { call(["rules", "put", "--data", JSON.stringify(data)], done); }
    function doctor(done) { call(["doctor"], done); }
    function deckDevices(done) { call(["deck", "devices"], done); }
    function deckPair(done) { call(["deck", "pair"], done); }
    function deckUnpair(name, done) { call(["deck", "unpair", name], done); }
    function deckButtons(done) { call(["deck", "buttons"], done); }
    function deckPut(buttons, done) { call(["deck", "put", "--data", JSON.stringify({ button: buttons })], done); }
    function deckReset(done) { call(["deck", "reset"], done); }

    Component {
        id: callComp
        Process {
            id: cp
            property var done: null
            stdout: StdioCollector {
                onStreamFinished: {
                    const r = svc.parse(text);
                    const d = cp.done;
                    svc.callsRunning = Math.max(0, svc.callsRunning - 1);
                    cp.destroy(50);
                    if (d)
                        d(r);
                }
            }
        }
    }

    // Applying display changes uses the user's confirm window.
    function withTimeout(args) {
        const a = args[0];
        const timed = a === "set" || a === "layout" || a === "arrange" || (a === "mode" && args[1] === "apply");
        return timed && args.indexOf("--timeout") < 0 ? args.concat(["--timeout", String(prefs.confirmTimeout)]) : args;
    }

    function refresh() {
        listProc.running = true;
        modesProc.running = true;
        quickProc.running = true;
    }

    // Night light / brightness / audio: no confirm timer, so they run on
    // their own process and never block display changes.
    function runQuick(args) {
        if (quickActProc.running) {
            quickQueued = args;
            return;
        }
        quickActProc.command = [svc.bin].concat(args).concat(["--json"]);
        quickActProc.running = true;
    }

    function run(args) {
        if (actProc.running)
            return;
        actProc.command = [svc.bin].concat(withTimeout(args)).concat(["--json"]);
        actProc.running = true;
    }

    function confirm() { run(["confirm"]); }
    function revert() { run(["revert"]); }
    function applyMode(name) { run(["mode", "apply", name]); }

    function parse(text) {
        try {
            return JSON.parse(text);
        } catch (e) {
            return null;
        }
    }

    Process {
        id: listProc
        command: [svc.bin, "monitors", "--json"]
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (r && r.ok)
                    svc.monitors = r.data;
                else if (r && r.error)
                    svc.lastError = r.error.message;
            }
        }
    }

    Process {
        id: modesProc
        command: [svc.bin, "mode", "list", "--json"]
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (r && r.ok) {
                    svc.modes = r.data.modes;
                    svc.activeMode = r.data.active || "";
                }
            }
        }
    }

    Process {
        id: quickProc
        command: [svc.bin, "quick", "--json"]
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (r && r.ok)
                    svc.quick = r.data;
                if (svc.quickDirty) {
                    svc.quickDirty = false;
                    quickProc.running = true;
                }
            }
        }
    }

    Process {
        id: quickActProc
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (!r)
                    svc.quickError = StanceI18n.t("stance returned no output");
                else if (!r.ok)
                    svc.quickError = r.error.message;
                else
                    svc.quickError = (r.data && r.data.warnings && r.data.warnings.length) ? r.data.warnings.join("\n") : "";
                if (svc.quickQueued) {
                    const next = svc.quickQueued;
                    svc.quickQueued = null;
                    Qt.callLater(() => svc.runQuick(next));
                }
                if (quickProc.running)
                    svc.quickDirty = true;
                else
                    quickProc.running = true;
            }
        }
    }

    Process {
        id: actProc
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (!r) {
                    svc.lastError = StanceI18n.t("stance returned no output");
                } else if (!r.ok) {
                    svc.lastError = r.error.message;
                } else {
                    svc.lastError = "";
                    svc.lastNote = (r.data && r.data.warnings && r.data.warnings.length) ? r.data.warnings.join("\n") : "";
                }
                svc.refresh();
                pendingFile.reload();
            }
        }
    }

    // Directory watch catches pending.json appearing / disappearing (it is
    // replaced atomically, so a watch on the file alone would go stale).
    // `stance status` creates the state directory, so the watch below has
    // something to watch even on a fresh install.
    property bool watching: false
    Process {
        id: initProc
        command: [svc.bin, "status", "--json"]
        running: true
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (r && r.ok && r.data)
                    svc.daemon = r.data.daemon === true;
            }
        }
        onExited: {
            svc.watching = true;
            pendingFile.reload();
        }
    }

    FolderListModel {
        folder: svc.watching ? "file://" + svc.stateDir.split("/").map(encodeURIComponent).join("/") : ""
        nameFilters: ["pending.json"]
        showDirs: false
        showDotAndDotDot: false
        onCountChanged: pendingFile.reload()
    }

    FileView {
        id: pendingFile
        path: svc.stateDir + "/pending.json"
        printErrors: false
        onLoaded: {
            const p = svc.parse(text());
            if (!p || (svc.session && p.session !== svc.session)) {
                svc.pending = null;
                return;
            }
            svc.pending = {
                label: p.label,
                deadline_ms: p.deadline_ms,
                timeout_ms: p.timeout_ms,
                generation: p.generation
            };
            svc.now = Date.now();
        }
        onLoadFailed: svc.pending = null
    }

    // Runs only while a change awaits confirmation.
    Timer {
        interval: 100
        repeat: true
        running: svc.pending !== null
        property int ticks: 0
        onTriggered: {
            svc.now = Date.now();
            // Re-read now and then: another change may have moved the deadline.
            if (++ticks % 10 === 0 || svc.remainingMs <= 0)
                pendingFile.reload();
            if (svc.remainingMs <= 0 && ticks % 10 === 0)
                svc.refresh();
        }
    }

}
