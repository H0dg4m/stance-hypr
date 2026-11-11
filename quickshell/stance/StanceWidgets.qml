pragma Singleton
import QtQuick
import Quickshell

// Widget manifests (docs/widgets.md). In-tree only: every widget wraps a
// stance feature, StanceBoard maps each id to its component. The board is
// stored in panel.json as an ordered list of {id, size}; an id that is not
// in the list is hidden. Titles are English i18n keys (StanceI18n.t).
Singleton {
    id: widgets

    readonly property int schema: 1

    // sizes: S = half-width tile, M = full width, one row, L = full card.
    // feature: `stance feature` gate (hidden while that feature is off).
    readonly property var list: [
        { id: "map", title: "Screen map", icon: "grid_view", sizes: ["M", "L"], defaultSize: "L", feature: null },
        { id: "screen", title: "Screen settings", icon: "monitor", sizes: ["L"], defaultSize: "L", feature: null },
        { id: "night", title: "Night light", icon: "bedtime", sizes: ["S", "M"], defaultSize: "M", feature: "quick" },
        { id: "brightness", title: "Brightness", icon: "light_mode", sizes: ["S", "M"], defaultSize: "M", feature: "quick" },
        { id: "audio", title: "Audio output", icon: "volume_up", sizes: ["S", "M"], defaultSize: "M", feature: "quick" },
        { id: "modes", title: "Modes", icon: "bookmarks", sizes: ["S", "M", "L"], defaultSize: "L", feature: null },
        { id: "clip", title: "Clipboard", icon: "content_paste", sizes: ["S"], defaultSize: "S", feature: "clip" },
        { id: "deck", title: "Phone", icon: "smartphone", sizes: ["S"], defaultSize: "S", feature: "deck" }
    ]
    readonly property var ids: list.map(w => w.id)

    function byId(id) {
        return list.find(w => w.id === id) || null;
    }

    readonly property var presetNames: ["minimal", "standard", "everything"]
    function preset(name) {
        if (name === "minimal")
            return [{ id: "map", size: "M" }, { id: "modes", size: "S" }, { id: "night", size: "S" }, { id: "audio", size: "S" }, { id: "brightness", size: "S" }];
        if (name === "everything")
            return list.map(w => ({ id: w.id, size: w.defaultSize }));
        // standard = the 0.4 panel
        return [{ id: "map", size: "L" }, { id: "screen", size: "L" }, { id: "night", size: "M" }, { id: "brightness", size: "M" }, { id: "audio", size: "M" }, { id: "modes", size: "L" }];
    }

    // Unknown ids and sizes a widget does not have are dropped (a size falls
    // back to the widget's default); duplicates keep the first.
    function normBoard(b) {
        const out = [];
        if (!Array.isArray(b))
            return out;
        for (const e of b) {
            const w = e && typeof e === "object" ? byId(e.id) : null;
            if (!w || out.some(o => o.id === w.id))
                continue;
            out.push({ id: w.id, size: w.sizes.indexOf(e.size) >= 0 ? e.size : w.defaultSize });
        }
        return out;
    }

    // panel.json schema 1 had `show.<part>` switches instead of a board.
    function fromShow(show) {
        const s = show && typeof show === "object" ? show : {};
        return preset("standard").filter(e => s[e.id] !== false);
    }
}
