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

    readonly property var selected: {
        const ms = service.monitors;
        return ms.find(m => m.name === selectedName) || ms.find(m => m.name === preferredName)
            || ms.find(m => m.focused) || ms[0] || null;
    }

    spacing: 8
    width: 360

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

    // ---- header ----------------------------------------------------------
    RowLayout {
        Layout.fillWidth: true
        Text {
            text: "Displays"
            color: panel.theme.text
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize + 3
            font.weight: Font.DemiBold
            Layout.fillWidth: true
        }
        StanceLabel {
            theme: panel.theme
            visible: panel.service.activeMode !== ""
            text: "mode · " + panel.service.activeMode
        }
    }

    // ---- layout map ------------------------------------------------------
    Rectangle {
        id: map
        Layout.fillWidth: true
        implicitHeight: 110
        radius: panel.theme.radius
        color: panel.theme.surface
        border.width: 1
        border.color: panel.theme.border

        readonly property var shown: panel.service.monitors.filter(m => m.enabled && !m.mirror_of)
        readonly property var box: {
            let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
            for (const m of shown) {
                const l = panel.logical(m);
                x0 = Math.min(x0, m.x); y0 = Math.min(y0, m.y);
                x1 = Math.max(x1, m.x + l.w); y1 = Math.max(y1, m.y + l.h);
            }
            return shown.length ? { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } : { x: 0, y: 0, w: 1, h: 1 };
        }
        readonly property real k: Math.min((width - 24) / box.w, (height - 24) / box.h)

        Repeater {
            model: map.shown
            delegate: Rectangle {
                required property var modelData
                readonly property var l: panel.logical(modelData)
                readonly property bool isSel: panel.selected && panel.selected.name === modelData.name
                x: (map.width - map.box.w * map.k) / 2 + (modelData.x - map.box.x) * map.k
                y: (map.height - map.box.h * map.k) / 2 + (modelData.y - map.box.y) * map.k
                width: Math.max(8, l.w * map.k - 3)
                height: Math.max(8, l.h * map.k - 3)
                radius: 4
                color: isSel ? panel.theme.accent : panel.theme.surfaceHover
                border.width: 1
                border.color: isSel ? panel.theme.accent : panel.theme.border
                Text {
                    anchors.centerIn: parent
                    width: parent.width - 4
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: parent.modelData.name
                    color: parent.isSel ? panel.theme.accentText : panel.theme.text
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize - 2
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: panel.selectedName = parent.modelData.name
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
                label: modelData.name + (modelData.enabled ? " (mirror)" : " (off)")
                selected: panel.selected && panel.selected.name === modelData.name
                onClicked: panel.selectedName = modelData.name
            }
        }
    }

    // ---- selected screen -------------------------------------------------
    ColumnLayout {
        id: detail
        visible: panel.selected !== null
        Layout.fillWidth: true
        spacing: 6
        readonly property var m: panel.selected
        readonly property var ref: m ? panel.others(m)[0] : null
        readonly property bool active: m && m.enabled && !m.mirror_of

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 4
            ColumnLayout {
                spacing: 0
                Layout.maximumWidth: 230
                Text {
                    text: detail.m ? detail.m.name : ""
                    color: panel.theme.text
                    font.family: panel.theme.fontFamily
                    font.pixelSize: panel.theme.fontSize + 1
                    font.weight: Font.DemiBold
                }
                StanceLabel {
                    theme: panel.theme
                    text: detail.m ? detail.m.description : ""
                    visible: text !== ""
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }
            Item { Layout.fillWidth: true }
            StanceChip {
                theme: panel.theme
                label: detail.m && detail.m.enabled ? "Turn off" : "Turn on"
                danger: true
                enabled: detail.m && !panel.service.busy && !(detail.active && panel.activeCount() <= 1)
                onClicked: panel.service.run(["set", detail.m.name, detail.m.enabled ? "--off" : "--on"])
            }
        }

        StanceChip {
            visible: detail.m && detail.m.enabled && !!detail.m.mirror_of
            theme: panel.theme
            label: "Stop mirroring " + (detail.m && detail.m.mirror_of ? detail.m.mirror_of : "")
            enabled: !panel.service.busy
            onClicked: panel.service.run(["layout", "right-of", detail.m.name, detail.m.mirror_of])
        }

        StanceLabel { theme: panel.theme; visible: detail.active; text: "Refresh rate" }
        Flow {
            visible: detail.active
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: detail.active ? panel.refreshRates(detail.m) : []
                delegate: StanceChip {
                    required property var modelData
                    theme: panel.theme
                    label: panel.fmtHz(modelData) + " Hz"
                    selected: Math.abs(modelData - detail.m.refresh) < 0.5
                    enabled: !panel.service.busy
                    onClicked: if (!selected) panel.service.run(["set", detail.m.name, "--hz", String(modelData)])
                }
            }
        }

        StanceLabel { theme: panel.theme; visible: detail.active; text: "Resolution" }
        Flow {
            visible: detail.active
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

        StanceLabel { theme: panel.theme; visible: detail.active; text: "Scale" }
        Flow {
            visible: detail.active
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: detail.active ? [1, 1.25, 1.5, 2] : []
                delegate: StanceChip {
                    required property var modelData
                    theme: panel.theme
                    label: modelData + "×"
                    selected: Math.abs(modelData - detail.m.scale) < 0.05
                    enabled: !panel.service.busy
                    onClicked: if (!selected) panel.service.run(["set", detail.m.name, "--scale", String(modelData)])
                }
            }
        }

        StanceLabel {

            theme: panel.theme
            visible: detail.active && !!detail.ref
            text: "Arrange relative to " + (detail.ref ? detail.ref.name : "")
        }
        Flow {
            visible: detail.active && !!detail.ref
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: [
                    { label: "Left", args: ["layout", "left-of"] },
                    { label: "Right", args: ["layout", "right-of"] },
                    { label: "Above", args: ["layout", "above"] },
                    { label: "Mirror", args: ["layout", "mirror"] }
                ]
                delegate: StanceChip {
                    required property var modelData
                    theme: panel.theme
                    label: modelData.label
                    enabled: !panel.service.busy
                    onClicked: panel.service.run(modelData.args.concat([detail.m.name, detail.ref.name]))
                }
            }
            StanceChip {
                theme: panel.theme
                label: "Only this one"
                enabled: !panel.service.busy
                onClicked: panel.service.run(["layout", "single", detail.m.name])
            }
        }
    }

    // ---- modes -----------------------------------------------------------
    Rectangle {
        Layout.fillWidth: true
        Layout.topMargin: 4
        implicitHeight: 1
        color: panel.theme.border
    }
    StanceLabel { theme: panel.theme; text: "Modes" }
    Flow {
        visible: panel.service.modes.length > 0
        Layout.fillWidth: true
        spacing: 6
        Repeater {
            model: panel.service.modes
            delegate: StanceChip {
                required property var modelData
                theme: panel.theme
                label: modelData.name
                selected: modelData.name === panel.service.activeMode
                enabled: !panel.service.busy && !modelData.error
                onClicked: panel.service.applyMode(modelData.name)
            }
        }
    }
    StanceLabel {
        theme: panel.theme
        visible: panel.service.modes.length === 0
        text: "No modes yet. Save the current setup with: stance mode save <name>"
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }

    Text {
        visible: text !== ""
        text: panel.service.lastError
        color: panel.theme.danger
        font.family: panel.theme.fontFamily
        font.pixelSize: panel.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    StanceLabel {
        theme: panel.theme
        visible: text !== "" && panel.service.lastError === ""
        text: panel.service.lastNote
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
