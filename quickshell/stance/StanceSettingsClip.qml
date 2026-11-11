import QtQuick
import QtQuick.Layouts

// Settings page: clipboard shelf configuration and capture pause.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

    property var checks: []
    function loadChecks() {
        service.doctor(r => {
            if (r && r.ok)
                checks = r.data;
        });
    }
    readonly property var clipCheck: checks.find(c => c.id === "clip") || null
    readonly property bool cliphistRuns: clipCheck !== null && /cliphist/i.test(clipCheck.message)

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

    Component.onCompleted: {
        loadClip();
        loadChecks();
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
