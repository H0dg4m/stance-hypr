import QtQuick
import QtQuick.Layouts

// Panel content: a clickable map of the screen layout, settings for the
// selected screen, and modes. Every click is one `stance …` call; the
// confirm overlay takes over from there.
ColumnLayout {
    id: panel

    required property StanceService service
    property StanceTheme theme: StanceTheme {}
    property string selectedName: ""
    // Screen the panel was opened on; selected until the user picks another.
    property string preferredName: ""
    // "main" | "mode:<name>"
    property string page: "main"
    // Settings live in their own window (StanceSettingsWindow).
    signal settingsRequested

    // UI preference with a default of true (missing or unreadable prefs).
    function pref(key) {
        const p = service.prefs;
        return !(p && p.show && p.show[key] === false);
    }

    readonly property var selected: {
        const ms = service.monitors;
        return ms.find(m => m.name === selectedName) || ms.find(m => m.name === preferredName)
            || ms.find(m => m.focused) || ms[0] || null;
    }

    spacing: 10
    width: 380

    function fmtHz(r) { return (Math.round(r * 100) / 100).toString(); }

    function logical(m) {
        const rot = m.transform % 2 === 1;
        const s = m.scale > 0 ? m.scale : 1;
        return { w: (rot ? m.height : m.width) / s, h: (rot ? m.width : m.height) / s };
    }

    function refreshRates(m) {
        const seen = {};
        const out = [];
        for (const md of m.modes) {
            if (md.width !== m.width || md.height !== m.height) continue;
            const k = Math.round(md.refresh);
            if (seen[k]) continue;
            seen[k] = true;
            out.push(md.refresh);
        }
        return out.sort((a, b) => b - a).slice(0, 6);
    }

    function resolutions(m) {
        const seen = {};
        const out = [];
        for (const md of m.modes) {
            const k = md.width + "x" + md.height;
            if (seen[k]) continue;
            seen[k] = true;
            out.push({ w: md.width, h: md.height });
        }
        out.sort((a, b) => b.w * b.h - a.w * a.h);
        const top = out.slice(0, 4);
        if (m.enabled && !top.some(r => r.w === m.width && r.h === m.height))
            top.push({ w: m.width, h: m.height });
        return top;
    }

    function others(m) {
        return service.monitors.filter(o => o.name !== m.name && o.enabled && !o.mirror_of);
    }

    function activeCount() {
        return service.monitors.filter(o => o.enabled && !o.mirror_of).length;
    }

    component RowLabel: Text {
        required property StanceTheme theme
        color: theme.textDim
        font.family: theme.fontFamily
        font.pixelSize: theme.fontSize - 1
        Layout.preferredWidth: 78
    }

    // ---- header ----------------------------------------------------------
    RowLayout {
        Layout.fillWidth: true
        Layout.leftMargin: 4
        Layout.rightMargin: 2
        spacing: 8
        StanceChip {
            visible: panel.page !== "main"
            theme: panel.theme
            icon: "arrow_back"
            label: panel.theme.iconFont === "" ? StanceI18n.t("Back") : ""
            // The mode editor may hold unsaved edits and ask first.
            onClicked: editorLoader.item && panel.page.startsWith("mode:") ? editorLoader.item.requestBack() : panel.page = "main"
        }
        StanceIcon { visible: panel.page === "main" && theme.iconFont !== ""; theme: panel.theme; name: "desktop_windows"; color: panel.theme.accent; fill: true }
        Text {
            text: panel.page.startsWith("mode:") ? StanceI18n.f("Mode · %1", panel.page.slice(5)) : StanceI18n.t("Displays")
            color: panel.theme.text
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize + 3
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }
        Rectangle {
            visible: panel.service.activeMode !== ""
            radius: height / 2
            color: panel.theme.accentSoft
            implicitHeight: 24
            implicitWidth: modeRow.implicitWidth + 16
            Row {
                id: modeRow
                anchors.centerIn: parent
                spacing: 4
                StanceIcon { theme: panel.theme; name: "bookmark"; size: panel.theme.fontSize + 1; color: panel.theme.accent; fill: true; anchors.verticalCenter: parent.verticalCenter }
                Text {
                    text: panel.service.activeMode
                    color: panel.theme.accent
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize - 2
                    font.weight: Font.Medium
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
        }
        StanceChip {
            visible: panel.page === "main" && panel.service.monitors.length > 1
            theme: panel.theme
            icon: "screen_search_desktop"
            label: panel.theme.iconFont === "" ? StanceI18n.t("Identify") : ""
            onClicked: identify.active = true
        }
        StanceChip {
            visible: panel.page === "main"
            theme: panel.theme
            icon: "settings"
            label: panel.theme.iconFont === "" ? StanceI18n.t("Settings") : ""
            onClicked: panel.settingsRequested()
        }
    }

    ColumnLayout {
    id: mainPage
    visible: panel.page === "main"
    Layout.fillWidth: true
    spacing: 10

    StanceIdentify {
        id: identify
        theme: panel.theme
        monitors: panel.service.monitors
    }

    // ---- layout map ------------------------------------------------------
    Rectangle {
        id: map
        visible: panel.pref("map")
        Layout.fillWidth: true
        implicitHeight: 150
        radius: panel.theme.radius
        color: panel.theme.card

        readonly property var live: panel.service.monitors.filter(m => m.enabled && !m.mirror_of)
        // Frozen while a screen is dragged, so a refresh cannot rebuild
        // the delegates under the pointer.
        property bool dragActive: false
        property var frozen: []
        readonly property var shown: dragActive ? frozen : live
        function beginDrag() { frozen = live; dragActive = true; }
        readonly property var box: {
            let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
            for (const m of shown) {
                const l = panel.logical(m);
                x0 = Math.min(x0, m.x); y0 = Math.min(y0, m.y);
                x1 = Math.max(x1, m.x + l.w); y1 = Math.max(y1, m.y + l.h);
            }
            if (!shown.length)
                return { x: 0, y: 0, w: 1, h: 1 };
            // Room around the layout so a screen can be dragged beside the
            // others (only when there is something to drag against).
            let pw = 0, ph = 0;
            if (shown.length > 1)
                for (const m of shown) {
                    const l = panel.logical(m);
                    pw = Math.max(pw, l.w * 0.25);
                    ph = Math.max(ph, l.h * 0.25);
                }
            return { x: x0 - pw, y: y0 - ph, w: x1 - x0 + 2 * pw, h: y1 - y0 + 2 * ph };
        }
        readonly property real k: Math.min((width - 32) / box.w, (height - 32) / box.h)
        readonly property real ox: (width - box.w * k) / 2
        readonly property real oy: (height - box.h * k) / 2
        clip: true

        // Dragging: snap the moved screen's edges to the others' edges
        // (within ~10 map px), then send every position in one "arrange" call.
        // x snaps only to screens beside it (y ranges meet), y only to
        // screens above/below or beside it after the x snap.
        function snap(name, x, y, w, h) {
            const t = 12 / k;
            let bx = x, dx = t;
            for (const o of shown) {
                if (o.name === name) continue;
                const l = panel.logical(o);
                if (!(y - t < o.y + l.h && o.y < y + h + t)) continue;
                for (const c of [o.x - w, o.x + l.w, o.x, o.x + l.w - w])
                    if (Math.abs(c - x) < dx) { dx = Math.abs(c - x); bx = c; }
            }
            let by = y, dy = t;
            for (const o of shown) {
                if (o.name === name) continue;
                const l = panel.logical(o);
                if (!(bx - t < o.x + l.w && o.x < bx + w + t)) continue;
                for (const c of [o.y - h, o.y + l.h, o.y, o.y + l.h - h])
                    if (Math.abs(c - y) < dy) { dy = Math.abs(c - y); by = c; }
            }
            return { x: Math.round(bx), y: Math.round(by) };
        }

        // Same rule as the CLI: shares an edge segment with another screen.
        function touches(name, x, y, w, h) {
            return shown.some(o => {
                if (o.name === name) return false;
                const l = panel.logical(o);
                const v = y < o.y + l.h && o.y < y + h;
                const hz = x < o.x + l.w && o.x < x + w;
                return (v && (x + w === o.x || o.x + l.w === x)) || (hz && (y + h === o.y || o.y + l.h === y));
            });
        }

        property string hint: ""
        Timer { id: hintTimer; interval: 2500; onTriggered: map.hint = "" }
        function showHint(t) { hint = t; hintTimer.restart(); }

        function overlaps(name, x, y, w, h) {
            return shown.some(o => {
                if (o.name === name) return false;
                const l = panel.logical(o);
                return x < o.x + l.w && o.x < x + w && y < o.y + l.h && o.y < y + h;
            });
        }

        function commit(name, x, y) {
            // Shift everything so the layout starts at 0,0.
            const pos = shown.map(o => o.name === name ? { n: o.name, x: x, y: y } : { n: o.name, x: o.x, y: o.y });
            const mx = Math.min(...pos.map(p => p.x)), my = Math.min(...pos.map(p => p.y));
            const args = pos.filter(p => {
                const o = shown.find(s => s.name === p.n);
                return p.x - mx !== o.x || p.y - my !== o.y;
            }).map(p => p.n + "=" + (p.x - mx) + "x" + (p.y - my));
            if (args.length)
                panel.service.run(["arrange"].concat(args));
        }

        Text {
            visible: map.hint !== ""
            text: map.hint
            anchors.bottom: parent.bottom
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottomMargin: 6
            z: 3
            color: panel.theme.danger
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize - 3
        }

        // Keyed by name: a refresh (new array) must not recreate delegates
        // and cancel a drag in progress.
        Repeater {
            model: map.shown.map(m => m.name)
            delegate: Rectangle {
                id: scr
                required property string modelData
                readonly property var mon: map.shown.find(m => m.name === modelData) || ({ name: modelData, x: 0, y: 0, width: 1, height: 1, refresh: 0, scale: 1, transform: 0 })
                readonly property var l: panel.logical(mon)
                readonly property bool isSel: panel.selected && panel.selected.name === modelData
                property bool dragging: false
                property real dragX: mon.x
                property real dragY: mon.y
                readonly property bool bad: dragging && (map.overlaps(modelData, dragX, dragY, l.w, l.h) || !map.touches(modelData, dragX, dragY, l.w, l.h))
                x: map.ox + ((dragging ? dragX : mon.x) - map.box.x) * map.k
                y: map.oy + ((dragging ? dragY : mon.y) - map.box.y) * map.k
                z: dragging ? 2 : 0
                opacity: dragging ? 0.85 : 1
                border.width: dragging ? 2 : 0
                border.color: bad ? panel.theme.danger : panel.theme.accent
                width: Math.max(10, l.w * map.k - 4)
                height: Math.max(10, l.h * map.k - 4)
                radius: Math.min(8, panel.theme.chipRadius)
                color: isSel ? panel.theme.accent : (hover.containsMouse ? panel.theme.surfaceHover : panel.theme.surface)
                Behavior on color { ColorAnimation { duration: panel.theme.animMs } }
                Column {
                    anchors.centerIn: parent
                    width: parent.width - 8
                    spacing: 1
                    Text {
                        width: parent.width
                        horizontalAlignment: Text.AlignHCenter
                        elide: Text.ElideRight
                        text: scr.modelData
                        color: scr.isSel ? panel.theme.accentText : panel.theme.text
                        font.family: panel.theme.fontFamily
                        font.pixelSize: panel.theme.fontSize - 2
                        font.weight: Font.DemiBold
                    }
                    Text {
                        width: parent.width
                        visible: scr.height > 40
                        horizontalAlignment: Text.AlignHCenter
                        elide: Text.ElideRight
                        text: scr.mon.width + "×" + scr.mon.height + " · " + Math.round(scr.mon.refresh) + " Hz"
                        color: scr.isSel ? panel.theme.accentText : panel.theme.textDim
                        opacity: 0.85
                        font.family: panel.theme.fontFamily
                        font.pixelSize: panel.theme.fontSize - 4
                    }
                }
                // The screen the mouse/keyboard is on.
                Rectangle {
                    visible: scr.mon.focused
                    width: 6; height: 6; radius: 3
                    anchors.top: parent.top; anchors.right: parent.right; anchors.margins: 5
                    color: scr.isSel ? panel.theme.accentText : panel.theme.accent
                }
                MouseArea {
                    id: hover
                    anchors.fill: parent
                    hoverEnabled: true
                    enabled: !panel.service.busy && panel.service.pending === null
                    cursorShape: scr.dragging ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                    property point start
                    onPressed: e => {
                        start = mapToItem(map, e.x, e.y);
                        scr.dragX = scr.mon.x;
                        scr.dragY = scr.mon.y;
                    }
                    onPositionChanged: e => {
                        if (!pressed) return;
                        const p = mapToItem(map, e.x, e.y);
                        const dx = p.x - start.x, dy = p.y - start.y;
                        if (!scr.dragging && Math.abs(dx) + Math.abs(dy) < 5) return;
                        if (!scr.dragging) {
                            if (map.shown.length < 2) return;
                            map.beginDrag();
                            scr.dragging = true;
                        }
                        const s = map.snap(scr.modelData, scr.mon.x + dx / map.k, scr.mon.y + dy / map.k, scr.l.w, scr.l.h);
                        scr.dragX = s.x;
                        scr.dragY = s.y;
                    }
                    onReleased: {
                        if (!scr.dragging) {
                            panel.selectedName = scr.modelData;
                            return;
                        }
                        const ok = !scr.bad;
                        scr.dragging = false;
                        map.dragActive = false;
                        panel.selectedName = scr.modelData;
                        if (ok)
                            map.commit(scr.modelData, scr.dragX, scr.dragY);
                        else
                            map.showHint(StanceI18n.t("Screens must touch and must not overlap"));
                    }
                    onCanceled: { scr.dragging = false; map.dragActive = false; }
                }
            }
        }
    }

    // Screens that are off or mirrored are not on the map; list them.
    Flow {
        Layout.fillWidth: true
        spacing: 6
        visible: panel.service.monitors.some(m => !m.enabled || m.mirror_of)
        Repeater {
            model: panel.service.monitors.filter(m => !m.enabled || m.mirror_of)
            delegate: StanceChip {
                required property var modelData
                theme: panel.theme
                icon: modelData.enabled ? "content_copy" : "desktop_access_disabled"
                label: StanceI18n.f(modelData.enabled ? "%1 · mirror" : "%1 · off", modelData.name)
                selected: panel.selected && panel.selected.name === modelData.name
                onClicked: panel.selectedName = modelData.name
            }
        }
    }

    // ---- selected screen -------------------------------------------------
    StanceSection {
        id: detail
        visible: panel.selected !== null && panel.pref("screen")
        Layout.fillWidth: true
        theme: panel.theme
        readonly property var m: panel.selected
        readonly property var ref: m ? panel.others(m)[0] : null
        readonly property bool active: m && m.enabled && !m.mirror_of

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            StanceIcon { theme: panel.theme; name: "monitor"; color: panel.theme.textDim }
            ColumnLayout {
                spacing: 0
                Layout.fillWidth: true
                Layout.preferredWidth: 1000
                Text {
                    text: detail.m ? detail.m.name : ""
                    color: panel.theme.text
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize + 1
                    font.weight: Font.DemiBold
                }
                Text {
                    text: detail.m ? detail.m.description : ""
                    visible: text !== ""
                    elide: Text.ElideRight
                    color: panel.theme.textDim
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize - 2
                    Layout.fillWidth: true
                }
            }
            StanceChip {
                theme: panel.theme
                icon: "power_settings_new"
                label: detail.m && detail.m.enabled ? StanceI18n.t("Turn off") : StanceI18n.t("Turn on")
                danger: detail.m && detail.m.enabled
                enabled: detail.m && !panel.service.busy && !(detail.active && panel.activeCount() <= 1)
                onClicked: panel.service.run(["set", detail.m.name, detail.m.enabled ? "--off" : "--on"])
            }
        }

        StanceChip {
            visible: detail.m && detail.m.enabled && !!detail.m.mirror_of
            theme: panel.theme
            icon: "close"
            label: StanceI18n.f("Stop mirroring %1", detail.m && detail.m.mirror_of ? detail.m.mirror_of : "")
            enabled: !panel.service.busy
            onClicked: panel.service.run(["layout", "right-of", detail.m.name, detail.m.mirror_of])
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: StanceI18n.t("Refresh") }
            StanceSegmented {
                Layout.fillWidth: true
                theme: panel.theme
                enabled: !panel.service.busy
                model: detail.active ? panel.refreshRates(detail.m).map(r => ({ label: panel.fmtHz(r), value: r })) : []
                current: detail.m ? detail.m.refresh : null
                same: (a, b) => Math.abs(a - b) < 0.5
                onPicked: v => panel.service.run(["set", detail.m.name, "--hz", String(v)])
            }
            Text {
                text: "Hz"
                color: panel.theme.textDim
                font.family: panel.theme.fontFamily
                font.pixelSize: panel.theme.fontSize - 2
            }
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: StanceI18n.t("Scale") }
            StanceSegmented {
                Layout.fillWidth: true
                theme: panel.theme
                enabled: !panel.service.busy
                model: [1, 1.25, 1.5, 2].map(v => ({ label: v + "×", value: v }))
                current: detail.m ? detail.m.scale : null
                same: (a, b) => Math.abs(a - b) < 0.05
                onPicked: v => panel.service.run(["set", detail.m.name, "--scale", String(v)])
            }
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            RowLabel { theme: panel.theme; text: StanceI18n.t("Resolution"); Layout.alignment: Qt.AlignTop; Layout.topMargin: 6 }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: detail.active ? panel.resolutions(detail.m) : []
                    delegate: StanceChip {
                        required property var modelData
                        theme: panel.theme
                        label: modelData.w + "×" + modelData.h
                        selected: modelData.w === detail.m.width && modelData.h === detail.m.height
                        enabled: !panel.service.busy
                        onClicked: if (!selected) panel.service.run(["set", detail.m.name, "--res", modelData.w + "x" + modelData.h])
                    }
                }
            }
        }

        RowLayout {
            visible: detail.active && !!detail.ref
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: StanceI18n.t("Place"); Layout.alignment: Qt.AlignTop; Layout.topMargin: 6 }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: [
                        { label: StanceI18n.t("Left"), icon: "west", args: ["layout", "left-of"] },
                        { label: StanceI18n.t("Right"), icon: "east", args: ["layout", "right-of"] },
                        { label: StanceI18n.t("Above"), icon: "north", args: ["layout", "above"] },
                        { label: StanceI18n.t("Mirror"), icon: "content_copy", args: ["layout", "mirror"] }
                    ]
                    delegate: StanceChip {
                        required property var modelData
                        theme: panel.theme
                        icon: modelData.icon
                        label: modelData.label
                        enabled: !panel.service.busy
                        onClicked: panel.service.run(modelData.args.concat([detail.m.name, detail.ref.name]))
                    }
                }
                StanceChip {
                    theme: panel.theme
                    icon: "crop_square"
                    label: StanceI18n.t("Only this")
                    enabled: !panel.service.busy
                    onClicked: panel.service.run(["layout", "single", detail.m.name])
                }
            }
        }
        Text {
            visible: detail.active && !!detail.ref
            text: StanceI18n.f("relative to %1", detail.ref ? detail.ref.name : "")
            color: panel.theme.textDim
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize - 3
            Layout.leftMargin: 84
            Layout.topMargin: -4
        }
    }

    StanceQuick {
        Layout.fillWidth: true
        service: panel.service
        theme: panel.theme
    }

    StanceModes {
        Layout.fillWidth: true
        visible: panel.pref("modes")
        service: panel.service
        theme: panel.theme
        onEdit: name => panel.page = "mode:" + name
    }

    }

    Loader {
        id: editorLoader
        Layout.fillWidth: true
        active: panel.page.startsWith("mode:")
        visible: active
        sourceComponent: StanceModeEditor {
            service: panel.service
            theme: panel.theme
            modeName: panel.page.slice(5)
            onBack: panel.page = "main"
        }
    }

    // ---- messages --------------------------------------------------------
    Rectangle {
        visible: panel.service.lastError !== "" || panel.service.lastNote !== ""
        Layout.fillWidth: true
        implicitHeight: msg.implicitHeight + 16
        radius: panel.theme.chipRadius
        readonly property bool isError: panel.service.lastError !== ""
        color: isError ? Qt.rgba(panel.theme.danger.r, panel.theme.danger.g, panel.theme.danger.b, 0.14) : panel.theme.card
        RowLayout {
            id: msg
            x: 10; y: 8
            width: parent.width - 20
            spacing: 8
            StanceIcon {
                theme: panel.theme
                name: parent.parent.isError ? "error" : "info"
                size: panel.theme.fontSize + 2
                color: parent.parent.isError ? panel.theme.danger : panel.theme.textDim
                Layout.alignment: Qt.AlignTop
            }
            Text {
                text: panel.service.lastError || panel.service.lastNote
                color: parent.parent.isError ? panel.theme.danger : panel.theme.textDim
                font.family: panel.theme.fontFamily
                font.pixelSize: panel.theme.fontSize - 2
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
}
