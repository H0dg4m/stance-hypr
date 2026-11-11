import QtQuick

// Colours and metrics. Every value is resolved in this order:
//   overrides (the user's tweaks) > named theme (apply()) > `shell` > default.
// `shell` is for embedding shells: pass the colours you want as long as the
// user has not picked a named theme, e.g.
//   StanceTheme { shell: ({ accent: Appearance.colors.colPrimary, radius: 16 }) }
// Plain `accent: ...` assignments still work (they replace the resolved value).
// The contract is docs/themes.md; StanceService calls apply() from panel.json.
QtObject {
    id: th

    // Keys of the theme contract.
    readonly property var colorKeys: ["background", "card", "surface", "surfaceHover", "border",
        "text", "textDim", "accent", "accentText", "danger", "dangerText", "scrim"]
    readonly property var metricKeys: ["radius", "chipRadius", "fontFamily", "fontSize",
        "iconFont", "animMs", "padding", "spacing", "opacity"]

    property var shell: null
    property var named: null
    property var overrides: ({})

    // `themeObject`: a full theme (`stance theme list --json` entry) or null for
    // "keep what the shell gave"; `ov`: { key: value } tweaks on top.
    function apply(themeObject, ov) {
        named = themeObject && typeof themeObject === "object" ? themeObject : null;
        overrides = ov && typeof ov === "object" ? ov : ({});
    }

    // One resolved value; `d` is the built-in default.
    function pick(k, d) {
        const o = overrides;
        if (o && o[k] !== undefined && o[k] !== null && o[k] !== "")
            return o[k];
        const n = named;
        if (n) {
            const v = n.colors && n.colors[k] !== undefined ? n.colors[k] : (n.metrics ? n.metrics[k] : undefined);
            if (v !== undefined && v !== "")
                return v;
            // A font a theme leaves empty means "the shell's / the default".
            if (k !== "fontFamily" && k !== "iconFont")
                return d;
        }
        const s = shell;
        if (s && s[k] !== undefined && s[k] !== null && s[k] !== "")
            return s[k];
        return d;
    }

    // The whole resolved theme as a contract object (for "Save as theme").
    function snapshot() {
        const colors = {};
        const defs = defaultColors;
        for (const k of colorKeys)
            colors[k] = hex(Qt.color(String(pick(k, defs[k]))));
        const metrics = {};
        for (const k of metricKeys)
            metrics[k] = pick(k, defaultMetrics[k]);
        // The built-in fallback font is not a choice: leave it empty (= default).
        if (metrics.fontFamily === defaultMetrics.fontFamily)
            metrics.fontFamily = "";
        return { colors: colors, metrics: metrics, dark: dark };
    }

    function hex(c) {
        const h = v => ("0" + Math.round(v * 255).toString(16)).slice(-2);
        return "#" + (c.a < 0.999 ? h(c.a) : "") + h(c.r) + h(c.g) + h(c.b);
    }

    readonly property var defaultColors: ({
        background: "#1e1f24", card: "#26282e", surface: "#2f323a", surfaceHover: "#3a3d47",
        border: "#3d404b", text: "#e6e7ec", textDim: "#9a9daa", accent: "#8ab4f8",
        accentText: "#10213a", danger: "#f28b82", dangerText: "#3b0b08", scrim: "#99000000"
    })
    readonly property var defaultMetrics: ({
        radius: 12, chipRadius: 8, fontFamily: "sans-serif", fontSize: 13, iconFont: "",
        animMs: 120, padding: 12, spacing: 8, opacity: 1.0
    })

    function col(k) { return Qt.color(String(pick(k, defaultColors[k]))); }
    function mul(c, a) { return Qt.rgba(c.r, c.g, c.b, c.a * a); }
    function solid(c) { return Qt.rgba(c.r, c.g, c.b, 1); }

    // `opacity` (0.5-1) applies to background and card only.
    property real opacity: Number(pick("opacity", 1))
    property color background: mul(col("background"), opacity)
    // Section cards sit on the background; chips and controls on the cards.
    property color card: mul(col("card"), opacity)
    // Overlays (clip shelf, confirm card, settings window, previews) draw on
    // these: never translucent, whatever the opacity or the shell's colours.
    readonly property color backgroundSolid: solid(background)
    readonly property color cardSolid: solid(card)
    property color surface: col("surface")
    property color surfaceHover: col("surfaceHover")
    property color border: col("border")
    property color text: col("text")
    property color textDim: col("textDim")
    property color accent: col("accent")
    property color accentText: col("accentText")
    // Soft accent for "on" tiles that should not shout.
    property color accentSoft: Qt.rgba(accent.r, accent.g, accent.b, 0.18)
    property color danger: col("danger")
    property color dangerText: col("dangerText")
    // Dimmed backdrop behind overlays.
    property color scrim: col("scrim")
    property real radius: Number(pick("radius", 12))
    property real chipRadius: Number(pick("chipRadius", 8))
    property string fontFamily: String(pick("fontFamily", "sans-serif"))
    property real fontSize: Number(pick("fontSize", 13))
    // A Material Symbols family ("Material Symbols Rounded") turns icons on;
    // empty keeps the components free of any icon font.
    property string iconFont: String(pick("iconFont", ""))
    property int animMs: Number(pick("animMs", 120))
    // Inner padding of section cards and gap between rows inside them.
    property real padding: Number(pick("padding", 12))
    property real spacing: Number(pick("spacing", 8))
    readonly property bool dark: (0.299 * solid(background).r + 0.587 * solid(background).g + 0.114 * solid(background).b) < 0.5
}
