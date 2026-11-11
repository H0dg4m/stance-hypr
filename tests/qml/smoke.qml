//@ pragma UseQApplication
import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

// Offscreen smoke test (tests/qml-smoke.sh copies this next to the stance
// QML as shell.qml). Builds the panel and every settings page, saves a PNG
// of each into $STANCE_SMOKE_OUT and quits; the script fails on QML errors.
ShellRoot {
    id: root

    readonly property string out: Quickshell.env("STANCE_SMOKE_OUT")
    readonly property var steps: {
        // panel = first run (no panel.json: setup card on the standard board)
        const s = ["panel", "board:empty", "board:everything", "board:small", "board:medium"];
        // STANCE_SMOKE_STEPS=panel: only the first view (migration runs).
        if (Quickshell.env("STANCE_SMOKE_STEPS") === "panel")
            return ["panel"];
        for (const c of svc.settingsPages)
            s.push("settings:" + c);
        return s;
    }
    property int step: -1

    // Fixed quick data, so the tiles have something to show without tools.
    readonly property var fakeQuick: ({
        night: { on: true, temperature: 4500 },
        brightness: [{ monitor: "A-1", percent: 60, kind: "ddc" }, { monitor: "B-1", percent: 35, kind: "backlight" }],
        sinks: [
            { id: 1, name: "speakers", short: "Speakers", profile: "Analog", bus: "pci", virtual: false, default: true },
            { id: 2, name: "hdmi", short: "EPIC Q40", profile: "HDMI", bus: "pci", virtual: false, default: false }
        ],
        errors: []
    })
    function board(kind) {
        if (kind === "empty")
            return [];
        if (kind === "everything")
            return StanceWidgets.preset("everything");
        // every widget at its smallest / at M where it has one
        return StanceWidgets.list.map(w => ({ id: w.id, size: kind === "small" ? w.sizes[0] : (w.sizes.indexOf("M") >= 0 ? "M" : w.sizes[0]) }));
    }
    function prepare(name) {
        if (name.startsWith("board:")) {
            svc.quick = fakeQuick;
            svc.finishSetup(board(name.slice(6)));
        }
    }
    readonly property string current: step >= 0 && step < steps.length ? steps[step] : ""

    StanceService {
        id: svc
        bin: Quickshell.env("STANCE_BIN") || "stance"
        themeTarget: smokeTheme
        // The popup does this when it opens.
        Component.onCompleted: refresh()
        // What panel.json turned into, for the script to check.
        onPrefsStateChanged: if (prefsState !== "loading") console.info("smoke: prefs " + JSON.stringify({ state: prefsState, onboarded: prefs.onboarded, board: prefs.board.map(e => e.id + "." + e.size).join(",") }))
    }
    StanceClipService {
        id: clip
        bin: svc.bin
    }
    StanceTheme {
        id: smokeTheme
    }

    FloatingWindow {
        id: win
        implicitWidth: 420
        implicitHeight: 1400
        color: smokeTheme.background

        Rectangle {
            id: frame
            anchors.fill: parent
            // Grabs leave the window colour out; paint it.
            color: smokeTheme.background
            Loader {
                x: 20; y: 20
                width: 380
                active: root.current === "panel" || root.current.startsWith("board:")
                sourceComponent: StancePanel {
                    service: svc
                    theme: smokeTheme
                }
            }
            Loader {
                x: 20; y: 20
                width: 380
                active: root.current.startsWith("settings:")
                sourceComponent: StanceSettings {
                    width: 380
                    service: svc
                    theme: smokeTheme
                    cat: root.current.slice(9)
                }
            }
        }
    }

    // Let the CLI calls settle, capture, then move on (the grab is async:
    // advance only once it is saved).
    Timer {
        id: tick
        interval: 1500
        running: true
        onTriggered: {
            if (root.current === "") {
                root.advance();
                return;
            }
            const name = root.current.replace(":", "-");
            frame.grabToImage(r => {
                r.saveToFile(root.out + "/" + name + ".png");
                console.info("smoke: built " + root.current);
                root.advance();
            });
        }
    }
    function advance() {
        step++;
        if (step >= steps.length) {
            console.info("smoke: done");
            Qt.quit();
        } else {
            prepare(current);
            tick.restart();
        }
    }
}
