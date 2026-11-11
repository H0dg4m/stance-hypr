import QtQuick
import QtQuick.Layouts
import Quickshell

// Settings: language, theme picker, detailed editor (live: every change is
// applied at once and stored as `themeOverrides` in panel.json), save /
// import / export of theme files (`stance theme ...`, docs/themes.md).
ColumnLayout {
    id: ap

    required property StanceService service
    required property StanceTheme theme
    spacing: 10

    readonly property var prefs: service.prefs
    readonly property bool hasShell: service.defaultTheme === "shell"
    readonly property string homeDir: Quickshell.env("HOME") || ""
    readonly property var entry: service.activeThemeEntry
    readonly property bool userTheme: entry !== null && entry.builtin !== true
    readonly property int overrideCount: Object.keys(prefs.themeOverrides).length
    function overridesIn(keys) { return keys.filter(k => prefs.themeOverrides[k] !== undefined).length; }
    readonly property int colourOverrides: overridesIn(colorRows.map(r => r.key))
    readonly property int sizeOverrides: overridesIn(metricRows.map(r => r.key).concat(["opacity"]))
    readonly property int fontOverrides: overridesIn(["fontFamily", "iconFont"])
    property bool customising: false
    // Open group of the editor: "" | "colours" | "sizes" | "fonts"
    property string group: ""
    // "" | "save" | "export" | "import"
    property string action: ""
    property string status: ""
    property bool statusError: false
    property bool canForce: false
    property bool deleteAsk: false

    readonly property var colorRows: [
        { key: "background", label: "Background" }, { key: "card", label: "Card" },
        { key: "surface", label: "Surface" }, { key: "surfaceHover", label: "Surface (hover)" },
        { key: "border", label: "Border" }, { key: "text", label: "Text" },
        { key: "textDim", label: "Dim text" }, { key: "accent", label: "Accent" },
        { key: "accentText", label: "Text on accent" }, { key: "danger", label: "Danger" },
        { key: "dangerText", label: "Text on danger" }, { key: "scrim", label: "Backdrop" }
    ]
    readonly property var metricRows: [
        { key: "radius", label: "Corner radius" }, { key: "chipRadius", label: "Chip radius" },
        { key: "fontSize", label: "Font size" }, { key: "padding", label: "Padding" },
        { key: "spacing", label: "Spacing" }, { key: "animMs", label: "Animation (ms)" }
    ]

    // Colours of a theme by id (for the Automatic tile); the stock pair when
    // the id is not (or no longer) in the list.
    function themeColors(id, dark) {
        const e = service.themes.find(x => x.theme.id === id) || service.themes.find(x => x.theme.id === (dark ? "stance-dark" : "stance-light"));
        return e ? e.theme.colors : theme.defaultColors;
    }
    // Themes offered for one side of the pair: matching light/dark, plus the
    // current pick if it is something else.
    function pairChoices(dark) {
        const cur = dark ? prefs.themeDark : prefs.themeLight;
        return service.themes.filter(e => e.theme.dark === dark || e.theme.id === cur).map(e => ({ id: e.theme.id, name: e.theme.name }));
    }
    function slug(s) {
        return s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40);
    }
    function expand(p) {
        const t = p.trim();
        return t === "~" ? homeDir : t.startsWith("~/") ? homeDir + t.slice(1) : t;
    }
    function say(msg, err) { status = msg; statusError = err === true; }
    function open(which) {
        action = action === which ? "" : which;
        canForce = false;
        status = "";
        deleteAsk = false;
        if (action === "export")
            exportPath.text = "~/stance-theme-" + (entry ? entry.theme.id : "custom") + ".json";
        if (action === "save")
            saveName.text = "";
    }
    function fullTheme(name, id) {
        const snap = theme.snapshot();
        return { stance_theme: 1, id: id, name: name, dark: snap.dark, colors: snap.colors, metrics: snap.metrics };
    }
    function save(force) {
        const name = saveName.text.trim();
        const id = slug(name);
        if (name === "" || name.length > 60 || id === "" || id === "shell" || id === "auto") {
            say(StanceI18n.t("Give the theme a name (letters and digits)."), true);
            return;
        }
        service.putTheme(fullTheme(name, id), force, r => {
            if (r.ok) {
                service.pickTheme(id);
                action = "";
                canForce = false;
                say(StanceI18n.f("Saved as “%1”.", name), false);
            } else {
                canForce = /exist|already|overwrite|force/i.test(r.error + " " + r.code);
                say(r.error, true);
            }
        });
    }
    function doExport(force) {
        if (!entry) {
            say(StanceI18n.t("Save the colours as a theme first."), true);
            return;
        }
        const p = expand(exportPath.text);
        if (p === "") { say(StanceI18n.t("Enter a file path."), true); return; }
        service.exportTheme(entry.theme.id, p, force, r => {
            if (r.ok) { canForce = false; say(StanceI18n.f("Exported to %1", p), false); }
            else { canForce = /exist|already|overwrite|force/i.test(r.error + " " + r.code); say(r.error, true); }
        });
    }
    function doImport(force) {
        const p = expand(importPath.text);
        if (p === "") { say(StanceI18n.t("Enter a file path."), true); return; }
        service.importTheme(p, force, r => {
            if (r.ok) { canForce = false; importPath.text = ""; action = ""; say(StanceI18n.t("Imported."), false); }
            else { canForce = /exist|already|overwrite|force/i.test(r.error + " " + r.code); say(r.error, true); }
        });
    }
    function doDelete() {
        if (!userTheme)
            return;
        if (!deleteAsk) { deleteAsk = true; return; }
        deleteAsk = false;
        service.removeTheme(entry.theme.id, r => say(r.ok ? StanceI18n.t("Theme deleted.") : r.error, !r.ok));
    }

    component Dim: Text {
        color: ap.theme.textDim
        font.family: ap.theme.fontFamily
        font.pixelSize: ap.theme.fontSize - 1
        wrapMode: Text.Wrap
    }
    // WCAG contrast of the pairs that matter, from the live (override-aware) colours.
    function lum(c) {
        const f = v => v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
        return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
    }
    function ratio(a, b) {
        const x = lum(a), y = lum(b);
        return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
    }
    readonly property var contrastBad: {
        const pairs = [["text", "background", 4.5], ["text", "card", 4.5], ["textDim", "card", 3],
                       ["accentText", "accent", 4.5], ["dangerText", "danger", 4.5]];
        const out = [];
        for (const p of pairs) {
            const r = ratio(theme.col(p[0]), theme.col(p[1]));
            if (r < p[2])
                out.push(p[0] + " / " + p[1] + " " + r.toFixed(1) + ":1");
        }
        return out;
    }

    // Header of a collapsible editor group; `count` = overridden values in it.
    component GroupHead: Rectangle {
        id: gh
        property string key: ""
        property string title: ""
        property int count: 0
        readonly property bool on: ap.group === key
        Layout.fillWidth: true
        implicitHeight: 34
        radius: ap.theme.chipRadius
        color: gma.containsMouse ? ap.theme.surfaceHover : ap.theme.surface
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 8
            Text {
                text: gh.title
                color: ap.theme.text
                font.family: ap.theme.fontFamily
                font.pixelSize: ap.theme.fontSize
                font.weight: Font.Medium
                Layout.fillWidth: true
            }
            Text {
                visible: gh.count > 0
                text: StanceI18n.f("%1 changed", gh.count)
                color: ap.theme.accent
                font.family: ap.theme.fontFamily
                font.pixelSize: ap.theme.fontSize - 2
            }
            StanceIcon { theme: ap.theme; name: gh.on ? "expand_less" : "expand_more"; color: ap.theme.textDim }
            Text {
                visible: ap.theme.iconFont === ""
                text: gh.on ? "▴" : "▾"
                color: ap.theme.textDim
                font.pixelSize: ap.theme.fontSize
            }
        }
        MouseArea {
            id: gma
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: ap.group = gh.on ? "" : gh.key
        }
    }
    component Body: Text {
        color: ap.theme.text
        font.family: ap.theme.fontFamily
        font.pixelSize: ap.theme.fontSize
    }

    // ---- appearance ------------------------------------------------------------------
    StanceSection {
        Layout.fillWidth: true
        theme: ap.theme
        icon: "palette"
        title: StanceI18n.t("Theme")
        trailing: [
            StanceChip {
                theme: ap.theme
                icon: "restart_alt"
                label: StanceI18n.t("Reset overrides")
                enabled: ap.overrideCount > 0
                onClicked: ap.service.resetOverrides()
            }
        ]

        // Automatic: light theme by day, dark by night, whatever the system says.
        Rectangle {
            id: autoTile
            readonly property var lightCs: ap.themeColors(ap.prefs.themeLight, false)
            readonly property var darkCs: ap.themeColors(ap.prefs.themeDark, true)
            readonly property bool on: ap.service.autoTheme
            Layout.fillWidth: true
            implicitHeight: 44
            radius: ap.theme.chipRadius
            color: darkCs.background
            border.width: on ? 2 : 1
            border.color: on ? ap.theme.accent : ap.theme.border
            clip: true
            Rectangle {
                anchors.left: parent.left
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                width: parent.width / 2
                color: autoTile.lightCs.background
            }
            Row {
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.leftMargin: 10
                spacing: 4
                Repeater {
                    model: [autoTile.lightCs.accent, autoTile.lightCs.text, autoTile.darkCs.accent, autoTile.darkCs.text]
                    delegate: Rectangle {
                        required property string modelData
                        width: 14; height: 14; radius: 7
                        color: modelData
                        border.width: 1
                        border.color: Qt.rgba(0.5, 0.5, 0.5, 0.4)
                    }
                }
            }
            Rectangle {
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                height: 22
                width: autoText.implicitWidth + 16
                radius: 11
                color: ap.theme.surface
                Text {
                    id: autoText
                    anchors.centerIn: parent
                    textFormat: Text.PlainText
                    text: StanceI18n.t("Automatic (follow system)")
                    color: ap.theme.text
                    font.family: ap.theme.fontFamily
                    font.pixelSize: ap.theme.fontSize - 1
                    font.weight: autoTile.on ? Font.DemiBold : Font.Normal
                }
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: ap.service.pickTheme("auto")
            }
        }
        // The pair used by Automatic: light themes for light, dark for dark.
        ColumnLayout {
            visible: ap.service.autoTheme
            Layout.fillWidth: true
            spacing: 6
            Dim {
                Layout.fillWidth: true
                text: ap.service.systemScheme === "unknown"
                    ? StanceI18n.t("The system does not say whether it is light or dark; the default theme is used.")
                    : ap.service.systemScheme === "dark" ? StanceI18n.t("The system is dark now.") : StanceI18n.t("The system is light now.")
            }
            Dim { text: StanceI18n.t("Light theme"); Layout.fillWidth: true }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: ap.pairChoices(false)
                    delegate: StanceChip {
                        required property var modelData
                        theme: ap.theme
                        label: modelData.name
                        maxWidth: 200
                        selected: ap.prefs.themeLight === modelData.id
                        onClicked: ap.service.setPref("themeLight", modelData.id)
                    }
                }
            }
            Dim { text: StanceI18n.t("Dark theme"); Layout.fillWidth: true }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: ap.pairChoices(true)
                    delegate: StanceChip {
                        required property var modelData
                        theme: ap.theme
                        label: modelData.name
                        maxWidth: 200
                        selected: ap.prefs.themeDark === modelData.id
                        onClicked: ap.service.setPref("themeDark", modelData.id)
                    }
                }
            }
        }
        Dim {
            visible: ap.service.prefsSchema > 1
            Layout.fillWidth: true
            text: StanceI18n.t("panel.json comes from a newer stance, so changes here are not saved.")
            color: ap.theme.danger
        }

        // Theme tiles: a miniature of the theme in its own colours.
        Grid {
            id: tiles
            Layout.fillWidth: true
            columns: 3
            spacing: 8
            readonly property real tileW: Math.floor((width - 2 * spacing) / 3)
            Repeater {
                model: (ap.hasShell ? [{ shell: true, builtin: true, theme: { id: "shell", name: "Shell" } }] : []).concat(ap.service.themes)
                delegate: Rectangle {
                    id: tile
                    required property var modelData
                    readonly property var th: modelData.theme
                    readonly property var cs: modelData.shell ? (ap.theme.shell || ap.theme.defaultColors) : th.colors
                    readonly property bool on: !ap.service.autoTheme && ap.service.activeThemeId === th.id
                    width: tiles.tileW
                    height: 66
                    radius: ap.theme.chipRadius
                    color: cs.background
                    border.width: on ? 2 : 1
                    border.color: on ? ap.theme.accent : ap.theme.border
                    Column {
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 6
                        Row {
                            spacing: 4
                            Repeater {
                                model: ["card", "surface", "accent", "danger", "text"]
                                delegate: Rectangle {
                                    required property string modelData
                                    width: 14; height: 14; radius: 7
                                    color: tile.cs[modelData] || "transparent"
                                    border.width: 1
                                    border.color: Qt.rgba(1, 1, 1, 0.15)
                                }
                            }
                        }
                        Text {
                            width: parent.width
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                            maximumLineCount: 2
                            elide: Text.ElideRight
                            text: tile.modelData.shell ? StanceI18n.t("Shell") : tile.th.name
                            color: tile.cs.text || ap.theme.text
                            font.family: ap.theme.fontFamily
                            font.pixelSize: ap.theme.fontSize - 1
                            font.weight: tile.on ? Font.DemiBold : Font.Normal
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: ap.service.pickTheme(tile.th.id)
                    }
                }
            }
        }
        Dim {
            visible: ap.service.themeError !== ""
            text: ap.service.themeError
            color: ap.theme.danger
            Layout.fillWidth: true
        }
        Dim {
            visible: ap.service.themes.length === 0 && ap.service.themeError === ""
            text: StanceI18n.t("Loading…")
        }

        // Actions
        Flow {
            Layout.fillWidth: true
            spacing: 6
            StanceChip {
                theme: ap.theme
                icon: ap.customising ? "expand_less" : "tune"
                label: StanceI18n.t("Customise")
                selected: ap.customising
                onClicked: ap.customising = !ap.customising
            }
            StanceChip {
                theme: ap.theme
                icon: "save"
                label: StanceI18n.t("Save as theme…")
                selected: ap.action === "save"
                onClicked: ap.open("save")
            }
            StanceChip {
                theme: ap.theme
                icon: "file_download"
                label: StanceI18n.t("Import")
                selected: ap.action === "import"
                onClicked: ap.open("import")
            }
            StanceChip {
                theme: ap.theme
                icon: "file_upload"
                label: StanceI18n.t("Export")
                selected: ap.action === "export"
                enabled: ap.entry !== null
                onClicked: ap.open("export")
            }
            StanceChip {
                visible: ap.userTheme
                theme: ap.theme
                icon: "delete"
                label: ap.deleteAsk ? StanceI18n.t("Sure?") : StanceI18n.t("Delete theme")
                danger: true
                onClicked: ap.doDelete()
            }
        }

        // Save as
        ColumnLayout {
            visible: ap.action === "save"
            Layout.fillWidth: true
            spacing: 6
            Dim { text: StanceI18n.t("Saves the colours and sizes shown now as a new theme."); Layout.fillWidth: true }
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                StanceField {
                    id: saveName
                    Layout.fillWidth: true
                    theme: ap.theme
                    placeholder: StanceI18n.t("Theme name")
                    maxLength: 60
                    onAccepted: ap.save(false)
                }
                StanceChip {
                    theme: ap.theme
                    icon: "check"
                    label: StanceI18n.t("Save")
                    selected: true
                    enabled: saveName.text.trim() !== "" && !ap.service.callBusy
                    onClicked: ap.save(false)
                }
            }
            Dim {
                visible: saveName.text.trim() !== ""
                text: "id: " + ap.slug(saveName.text)
                font.pixelSize: ap.theme.fontSize - 3
            }
        }
        // Import
        RowLayout {
            visible: ap.action === "import"
            Layout.fillWidth: true
            spacing: 6
            StanceField {
                id: importPath
                Layout.fillWidth: true
                theme: ap.theme
                placeholder: StanceI18n.t("Path to a theme file")
                maxLength: 400
                onAccepted: ap.doImport(false)
            }
            StanceChip {
                theme: ap.theme
                icon: "check"
                label: StanceI18n.t("Import")
                selected: true
                enabled: importPath.text.trim() !== "" && !ap.service.callBusy
                onClicked: ap.doImport(false)
            }
        }
        // Export
        RowLayout {
            visible: ap.action === "export"
            Layout.fillWidth: true
            spacing: 6
            StanceField {
                id: exportPath
                Layout.fillWidth: true
                theme: ap.theme
                placeholder: StanceI18n.t("Where to write the file")
                maxLength: 400
                onAccepted: ap.doExport(false)
            }
            StanceChip {
                theme: ap.theme
                icon: "check"
                label: StanceI18n.t("Export")
                selected: true
                enabled: exportPath.text.trim() !== "" && !ap.service.callBusy
                onClicked: ap.doExport(false)
            }
        }
        RowLayout {
            visible: ap.status !== ""
            Layout.fillWidth: true
            spacing: 8
            Dim {
                text: ap.status
                textFormat: Text.PlainText
                color: ap.statusError ? ap.theme.danger : ap.theme.accent
                Layout.fillWidth: true
            }
            StanceChip {
                visible: ap.canForce
                theme: ap.theme
                label: StanceI18n.t("Overwrite")
                danger: true
                onClicked: ap.action === "save" ? ap.save(true) : ap.action === "import" ? ap.doImport(true) : ap.doExport(true)
            }
        }

        // ---- detailed editor ------------------------------------------------------
        ColumnLayout {
            visible: ap.customising
            Layout.fillWidth: true
            spacing: 6

            GroupHead { key: "colours"; title: StanceI18n.t("Colours"); count: ap.colourOverrides }
            ColumnLayout {
            visible: ap.group === "colours"
            Layout.fillWidth: true
            spacing: 6
            Dim {
                visible: ap.contrastBad.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: ap.theme.danger
                font.pixelSize: ap.theme.fontSize - 1
                text: StanceI18n.t("Low contrast") + ": " + ap.contrastBad.join(", ")
            }
            Repeater {
                model: ap.colorRows
                delegate: RowLayout {
                    id: crow
                    required property var modelData
                    readonly property color cur: ap.theme.col(modelData.key)
                    Layout.fillWidth: true
                    spacing: 8
                    Rectangle {
                        implicitWidth: 24
                        implicitHeight: 24
                        radius: 6
                        color: crow.cur
                        border.width: 1
                        border.color: ap.theme.border
                    }
                    Body {
                        text: StanceI18n.t(crow.modelData.label)
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        font.pixelSize: ap.theme.fontSize - 1
                        font.weight: ap.prefs.themeOverrides[crow.modelData.key] !== undefined ? Font.DemiBold : Font.Normal
                    }
                    StanceField {
                        id: hexField
                        implicitWidth: 112
                        implicitHeight: 30
                        theme: ap.theme
                        maxLength: 9
                        label: StanceI18n.t(crow.modelData.label)
                        text: ap.theme.hex(crow.cur)
                        invalid: !ap.service.isHex(text)
                        onEdited: v => { if (ap.service.isHex(v)) ap.service.setOverride(crow.modelData.key, v); }
                    }
                }
            }

            }

            GroupHead { key: "sizes"; title: StanceI18n.t("Sizes"); count: ap.sizeOverrides }
            ColumnLayout {
            visible: ap.group === "sizes"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: ap.metricRows
                delegate: RowLayout {
                    id: mrow
                    required property var modelData
                    readonly property var spec: ap.service.metricSpec[modelData.key]
                    Layout.fillWidth: true
                    spacing: 8
                    Body {
                        text: StanceI18n.t(mrow.modelData.label)
                        font.pixelSize: ap.theme.fontSize - 1
                        Layout.preferredWidth: 110
                        font.weight: ap.prefs.themeOverrides[mrow.modelData.key] !== undefined ? Font.DemiBold : Font.Normal
                    }
                    StanceSlider {
                        Layout.fillWidth: true
                        theme: ap.theme
                        label: StanceI18n.t(mrow.modelData.label)
                        from: mrow.spec.min
                        to: mrow.spec.max
                        value: ap.theme[mrow.modelData.key]
                        onMoved: v => ap.service.setOverride(mrow.modelData.key, Math.round(v))
                        onCommitted: v => ap.service.setOverride(mrow.modelData.key, Math.round(v))
                    }
                    Body {
                        text: Math.round(ap.theme[mrow.modelData.key])
                        horizontalAlignment: Text.AlignRight
                        Layout.preferredWidth: 34
                        font.pixelSize: ap.theme.fontSize - 1
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Body { text: StanceI18n.t("Opacity"); font.pixelSize: ap.theme.fontSize - 1; Layout.preferredWidth: 110 }
                StanceSlider {
                    Layout.fillWidth: true
                    theme: ap.theme
                    label: StanceI18n.t("Opacity")
                    from: 50
                    to: 100
                    value: Math.round(ap.theme.opacity * 100)
                    onMoved: v => ap.service.setOverride("opacity", Math.round(v) / 100)
                    onCommitted: v => ap.service.setOverride("opacity", Math.round(v) / 100)
                }
                Body { text: Math.round(ap.theme.opacity * 100) + " %"; horizontalAlignment: Text.AlignRight; Layout.preferredWidth: 44; font.pixelSize: ap.theme.fontSize - 1 }
            }
            Dim {
                text: StanceI18n.t("Opacity applies to the panel background and cards only; overlays stay solid.")
                Layout.fillWidth: true
                font.pixelSize: ap.theme.fontSize - 2
            }
            }

            GroupHead { key: "fonts"; title: StanceI18n.t("Fonts"); count: ap.fontOverrides }
            ColumnLayout {
            visible: ap.group === "fonts"
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: [{ key: "fontFamily", label: "Font" }, { key: "iconFont", label: "Icon font" }]
                delegate: RowLayout {
                    id: frow
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 8
                    Body { text: StanceI18n.t(frow.modelData.label); font.pixelSize: ap.theme.fontSize - 1; Layout.preferredWidth: 110 }
                    StanceField {
                        Layout.fillWidth: true
                        implicitHeight: 30
                        theme: ap.theme
                        maxLength: 64
                        placeholder: StanceI18n.t("Default")
                        text: ap.theme[frow.modelData.key]
                        onAccepted: ap.service.setOverride(frow.modelData.key, text)
                    }
                }
            }
            Dim {
                text: StanceI18n.t("Press Enter to apply a font. Icons need a Material Symbols family such as “Material Symbols Rounded”.")
                Layout.fillWidth: true
                font.pixelSize: ap.theme.fontSize - 2
            }
            }
        }
    }
}
