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
    property real now: Date.now()
    readonly property real remainingMs: pending ? Math.max(0, pending.deadline_ms - now) : 0
    readonly property real timeoutMs: pending ? Math.max(1, pending.timeout_ms || 12000) : 1

    readonly property string stateDir: (Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")) + "/stance"
    readonly property string session: Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE") || ""

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
        actProc.command = [svc.bin].concat(args).concat(["--json"]);
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
