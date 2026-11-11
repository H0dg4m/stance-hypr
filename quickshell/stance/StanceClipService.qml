import QtQuick
import Quickshell
import Quickshell.Io

// Data side of the clipboard shelf. Talks to `stance clip ... --json`; idle
// cost is zero. The list is loaded when the shelf opens (`active`), and while
// it is open `$XDG_STATE_HOME/stance/clip.rev` is watched so new copies show
// up without polling.
Item {
    id: svc

    // Same binary the main service uses (STANCE_BIN overrides it in shell.qml).
    property string bin: "stance"
    // The shelf is open: load, and keep reloading on clip.rev changes.
    property bool active: false

    property var items: []
    property bool capture: false
    property bool loaded: false
    property string error: ""
    property string note: ""
    property string query: ""
    property int rev: -1
    // Capture pause (stance clip pause): unix seconds from `list`, 0 = running.
    property real pausedUntil: 0
    property real now: Date.now()
    readonly property bool paused: pausedUntil * 1000 > now

    // Image paths are only trusted below this directory (STANCE_CLIP_DIR is
    // for tests with the fake CLI).
    readonly property string clipDir: Quickshell.env("STANCE_CLIP_DIR")
        || ((Quickshell.env("XDG_DATA_HOME") || (Quickshell.env("HOME") + "/.local/share")) + "/stance/clip")
    readonly property string revPath: Quickshell.env("STANCE_STATE_DIR") ? Quickshell.env("STANCE_STATE_DIR") + "/clip.rev" : (Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")) + "/stance/clip.rev"

    readonly property var images: items.filter(i => i.kind === "image")
    readonly property var texts: items.filter(i => i.kind === "text")
    readonly property var links: items.filter(i => i.kind === "link")

    function parse(text) {
        try { return JSON.parse(text); } catch (e) { return null; }
    }

    // True when `path` is a plain file path inside the clip directory.
    function safePath(path) {
        if (typeof path !== "string" || path === "")
            return false;
        const base = clipDir.replace(/\/+$/, "") + "/";
        return path.startsWith(base) && path.indexOf("/../") < 0 && path.indexOf("\0") < 0
            && /\.(png|jpe?g|webp|gif|bmp)$/i.test(path);
    }

    // Keep only what the shelf can show safely.
    function sanitize(list) {
        const out = [];
        for (const i of list) {
            if (!i || !/^[0-9]+$/.test(String(i.id)) || ["image", "text", "link"].indexOf(i.kind) < 0)
                continue;
            out.push({
                id: String(i.id),
                kind: i.kind,
                updated: Number(i.updated) || 0,
                pinned: i.pinned === true,
                preview: typeof i.preview === "string" ? i.preview : "",
                host: typeof i.host === "string" ? i.host : "",
                size: Number(i.size) || 0,
                mime: typeof i.mime === "string" ? i.mime : "",
                path: i.kind === "image" && svc.safePath(i.path) ? i.path : "",
                w: Number(i.w) || 0,
                h: Number(i.h) || 0
            });
        }
        return out;
    }

    function reload() {
        if (listProc.running)
            dirty = true;
        else
            listProc.running = true;
    }
    property bool dirty: false

    function setQuery(q) {
        query = q;
        reload();
    }

    // Mutations run one at a time; each one reloads the list when done.
    property var queue: []
    function mutate(args) {
        queue = queue.concat([args]);
        if (!actProc.running)
            next();
    }
    function next() {
        if (queue.length === 0)
            return;
        const a = queue[0];
        queue = queue.slice(1);
        actProc.command = [svc.bin, "clip"].concat(a).concat(["--json"]);
        actProc.running = true;
    }

    function copy(id) { mutate(["copy", id]); }
    function pin(id, on) { mutate([on ? "pin" : "unpin", id]); }
    function remove(id) { note = StanceI18n.t("Removed. Backspace undoes it."); mutate(["rm", id]); }
    function undo() { note = ""; mutate(["undo"]); }
    // for: "15m" | "1h" | "24h" (the CLI allows 24 h at most).
    function pause(duration) { mutate(["pause", duration]); }
    function resume() { mutate(["resume"]); }

    // Full text of one item for the preview (`done(string)`).
    function fullText(id, done) {
        const p = textComp.createObject(svc, { done: done, command: [svc.bin, "clip", "get", id] });
        p.running = true;
    }

    onActiveChanged: {
        if (active) {
            note = "";
            error = "";
            reload();
            revFile.reload();
        }
    }

    Process {
        id: listProc
        command: {
            const c = [svc.bin, "clip", "list", "--json", "--limit", "200"];
            if (svc.query !== "")
                c.push("--query", svc.query);
            return c;
        }
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (!r) {
                    svc.error = StanceI18n.t("stance clip returned no output (is stance new enough?)");
                } else if (!r.ok) {
                    svc.error = r.error ? r.error.message : StanceI18n.t("stance clip failed");
                } else {
                    svc.error = "";
                    svc.capture = r.data.enabled === true;
                    svc.rev = Number(r.data.rev) || 0;
                    svc.now = Date.now();
                    svc.pausedUntil = Number(r.data.paused_until) || 0;
                    svc.items = svc.sanitize(Array.isArray(r.data.items) ? r.data.items : []);
                }
                svc.loaded = true;
                if (svc.dirty) {
                    svc.dirty = false;
                    listProc.running = true;
                }
            }
        }
    }

    Process {
        id: actProc
        stdout: StdioCollector {
            onStreamFinished: {
                const r = svc.parse(text);
                if (!r)
                    svc.error = StanceI18n.t("stance clip returned no output");
                else if (!r.ok)
                    svc.error = r.error ? r.error.message : StanceI18n.t("stance clip failed");
                else
                    svc.error = "";
                svc.reload();
                svc.next();
            }
        }
    }

    Component {
        id: textComp
        Process {
            id: tp
            property var done: null
            stdout: StdioCollector {
                onStreamFinished: {
                    // Bounded: the preview never holds more than 64k chars.
                    const d = tp.done;
                    tp.destroy(50);
                    if (d)
                        d(text.length > 65536 ? text.slice(0, 65536) + "\n[…]" : text);
                }
            }
        }
    }

    // New copies while the shelf is open. The file is replaced atomically, so
    // watchChanges re-arms on each reload; debounce the burst.
    FileView {
        id: revFile
        path: svc.revPath
        watchChanges: svc.active
        printErrors: false
        onFileChanged: reload()
        onLoaded: {
            const n = parseInt(text().trim());
            if (svc.active && isFinite(n) && n !== svc.rev)
                debounce.restart();
        }
    }
    // The pause ends by itself: update `paused` at the deadline.
    Timer {
        interval: 5000; repeat: true; running: svc.active && svc.pausedUntil > 0
        onTriggered: svc.now = Date.now()
    }
    Timer { id: debounce; interval: 80; onTriggered: svc.reload() }
}
