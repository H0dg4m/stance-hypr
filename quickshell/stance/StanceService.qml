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
        compact: false
    })
    property var prefs: prefDefaults
    readonly property int confirmTimeout: prefs.confirmTimeout

    // Unknown keys, wrong types and out-of-range values fall back to defaults.
    function normPrefs(o) {
        const d = prefDefaults;
        const out = { show: {}, confirmTimeout: d.confirmTimeout, compact: d.compact };
        const show = o && typeof o.show === "object" && o.show ? o.show : {};
        for (const k of Object.keys(d.show))
            out.show[k] = typeof show[k] === "boolean" ? show[k] : d.show[k];
        if (o && typeof o.confirmTimeout === "number" && isFinite(o.confirmTimeout))
            out.confirmTimeout = Math.max(5, Math.min(60, Math.round(o.confirmTimeout)));
        if (o && typeof o.compact === "boolean")
            out.compact = o.compact;
        return out;
    }

    // key: "show.<name>" | "confirmTimeout" | "compact"
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
        prefs = normPrefs({});
        prefsSave.restart();
    }

    Process {
        id: mkdirProc
        command: ["mkdir", "-p", svc.configDir]
        running: true
        onExited: prefsFile.reload()
    }

    Timer {
        id: prefsSave
        interval: 500
        onTriggered: prefsFile.setText(JSON.stringify(svc.prefs, null, 1) + "\n")
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
                    svc.quickError = "stance returned no output";
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
                    svc.lastError = "stance returned no output";
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
        folder: svc.watching ? "file://" + svc.stateDir : ""
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
