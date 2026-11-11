pragma Singleton
import QtQml
import QtQuick
import Quickshell
import Quickshell.Io

// UI language. `lang` ("auto" | "en" | any code with an i18n/<code>.json file)
// comes from panel.json through StanceService. Strings are keyed by their
// English source text: t(s) gives the translation or, when there is none, s
// itself. f(s, a, b) fills %1, %2. Translations live in i18n/<code>.json
// (see docs/translating.md); a missing file just means English.
Singleton {
    id: i18n

    // Add a language: put i18n/<code>.json next to this file and list it here.
    readonly property var codes: ["tr"]

    property string lang: "auto"
    readonly property string envLang: Quickshell.env("LC_ALL") || Quickshell.env("LC_MESSAGES") || Quickshell.env("LANG") || ""
    // Loaded dictionaries by code, and their display names.
    property var dicts: ({})
    property var names: ({})
    // Available languages besides English: [{ code, name }].
    readonly property var languages: Object.keys(dicts).sort().map(c => ({ code: c, name: names[c] || c }))

    function _pick(raw) {
        // "pt_BR.UTF-8" -> "pt_BR", then "pt".
        const full = String(raw).split(".")[0].split("@")[0];
        if (dicts[full] !== undefined)
            return full;
        const base = full.split("_")[0];
        return dicts[base] !== undefined ? base : "en";
    }

    // The language actually used: "en" or a loaded code.
    readonly property string effective: lang === "en" ? "en" : lang === "auto" ? _pick(envLang) : (dicts[lang] !== undefined ? lang : "en")

    function t(s) {
        if (effective === "en")
            return s;
        const v = dicts[effective][s];
        return typeof v === "string" ? v : s;
    }

    function f(s, ...args) {
        let out = t(s);
        for (let i = 0; i < args.length; i++)
            out = out.split("%" + (i + 1)).join(String(args[i]));
        return out;
    }

    Instantiator {
        model: i18n.codes
        delegate: FileView {
            required property string modelData
            path: Qt.resolvedUrl("i18n/" + modelData + ".json").toString().replace(/^file:\/\//, "")
            onLoaded: {
                try {
                    const o = JSON.parse(text());
                    if (typeof o !== "object" || o === null)
                        return;
                    const nm = typeof o._name === "string" ? o._name : modelData;
                    delete o._name;
                    const d = Object.assign({}, i18n.dicts);
                    d[modelData] = o;
                    const n = Object.assign({}, i18n.names);
                    n[modelData] = nm;
                    i18n.names = n;
                    i18n.dicts = d;
                } catch (e) {
                    console.warn("stance i18n: bad " + modelData + ".json: " + e);
                }
            }
        }
    }
}
