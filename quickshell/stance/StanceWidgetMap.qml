import QtQuick
import QtQuick.Layouts

// Widget "map": the clickable, draggable screen layout, plus chips for
// screens that are off or mirrored. M is a shorter map. `panel` gives the
// selection and the geometry helpers (StancePanel).
ColumnLayout {
    id: mapWidget
    required property var panel
    property string size: "L"
    spacing: 10

    // ---- layout map ------------------------------------------------------
    Rectangle {
        id: map
        Layout.fillWidth: true
        implicitHeight: mapWidget.size === "M" ? 110 : 150
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
            textFormat: Text.PlainText
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
                        textFormat: Text.PlainText
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
                        textFormat: Text.PlainText
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
}
