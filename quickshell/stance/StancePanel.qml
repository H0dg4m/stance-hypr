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
        StanceIcon { theme: panel.theme; name: "desktop_windows"; color: panel.theme.accent; fill: true }
        Text {
            text: "Displays"
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
    }

    // ---- layout map ------------------------------------------------------
    Rectangle {
        id: map
        Layout.fillWidth: true
        implicitHeight: 124
        radius: panel.theme.radius
        color: panel.theme.card

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
        readonly property real k: Math.min((width - 32) / box.w, (height - 32) / box.h)

        Repeater {
            model: map.shown
            delegate: Rectangle {
                id: scr
                required property var modelData
                readonly property var l: panel.logical(modelData)
                readonly property bool isSel: panel.selected && panel.selected.name === modelData.name
                x: (map.width - map.box.w * map.k) / 2 + (modelData.x - map.box.x) * map.k
                y: (map.height - map.box.h * map.k) / 2 + (modelData.y - map.box.y) * map.k
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
                        text: scr.modelData.name
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
                        text: scr.modelData.width + "×" + scr.modelData.height + " · " + Math.round(scr.modelData.refresh) + " Hz"
                        color: scr.isSel ? panel.theme.accentText : panel.theme.textDim
                        opacity: 0.85
                        font.family: panel.theme.fontFamily
                        font.pixelSize: panel.theme.fontSize - 4
                    }
                }
                // The screen the mouse/keyboard is on.
                Rectangle {
                    visible: scr.modelData.focused
                    width: 6; height: 6; radius: 3
                    anchors.top: parent.top; anchors.right: parent.right; anchors.margins: 5
                    color: scr.isSel ? panel.theme.accentText : panel.theme.accent
                }
                MouseArea {
                    id: hover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: panel.selectedName = scr.modelData.name
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
                label: modelData.name + (modelData.enabled ? " · mirror" : " · off")
                selected: panel.selected && panel.selected.name === modelData.name
                onClicked: panel.selectedName = modelData.name
            }
        }
    }

    // ---- selected screen -------------------------------------------------
    StanceSection {
        id: detail
        visible: panel.selected !== null
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
                label: detail.m && detail.m.enabled ? "Turn off" : "Turn on"
                danger: detail.m && detail.m.enabled
                enabled: detail.m && !panel.service.busy && !(detail.active && panel.activeCount() <= 1)
                onClicked: panel.service.run(["set", detail.m.name, detail.m.enabled ? "--off" : "--on"])
            }
        }

        StanceChip {
            visible: detail.m && detail.m.enabled && !!detail.m.mirror_of
            theme: panel.theme
            icon: "close"
            label: "Stop mirroring " + (detail.m && detail.m.mirror_of ? detail.m.mirror_of : "")
            enabled: !panel.service.busy
            onClicked: panel.service.run(["layout", "right-of", detail.m.name, detail.m.mirror_of])
        }

        RowLayout {
            visible: detail.active
            Layout.fillWidth: true
            RowLabel { theme: panel.theme; text: "Refresh" }
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
            RowLabel { theme: panel.theme; text: "Scale" }
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
            RowLabel { theme: panel.theme; text: "Resolution"; Layout.alignment: Qt.AlignTop; Layout.topMargin: 6 }
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
            RowLabel { theme: panel.theme; text: "Place"; Layout.alignment: Qt.AlignTop; Layout.topMargin: 6 }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: [
                        { label: "Left", icon: "west", args: ["layout", "left-of"] },
                        { label: "Right", icon: "east", args: ["layout", "right-of"] },
                        { label: "Above", icon: "north", args: ["layout", "above"] },
                        { label: "Mirror", icon: "content_copy", args: ["layout", "mirror"] }
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
                    label: "Only this"
                    enabled: !panel.service.busy
                    onClicked: panel.service.run(["layout", "single", detail.m.name])
                }
            }
        }
        Text {
            visible: detail.active && !!detail.ref
            text: "relative to " + (detail.ref ? detail.ref.name : "")
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

    // ---- modes -----------------------------------------------------------
    StanceSection {
        Layout.fillWidth: true
        theme: panel.theme
        icon: "bookmarks"
        title: "Modes"
        Flow {
            visible: panel.service.modes.length > 0
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: panel.service.modes
                delegate: StanceChip {
                    required property var modelData
                    theme: panel.theme
                    icon: modelData.error ? "error" : "bookmark"
                    label: modelData.name
                    selected: modelData.name === panel.service.activeMode
                    enabled: !panel.service.busy && !modelData.error
                    onClicked: panel.service.applyMode(modelData.name)
                }
            }
        }
        Text {
            visible: panel.service.modes.length === 0
            text: "None yet · <font face='monospace'>stance mode save NAME</font>"
            textFormat: Text.StyledText
            color: panel.theme.textDim
            font.family: panel.theme.fontFamily
            font.pixelSize: panel.theme.fontSize - 2
            Layout.fillWidth: true
            wrapMode: Text.Wrap
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
