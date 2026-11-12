import QtQuick
import QtQuick.Layouts

// The widget board: `service.prefs.board` in order. S widgets are
// half-width tiles (two per row), M and L take the full width. A widget
// whose feature is off, or that has nothing to show (no night light tool,
// one audio output...), takes no space.
Item {
    id: board

    // The StancePanel: map and screen widgets use its selection and helpers.
    required property var host
    readonly property StanceService service: host.service
    readonly property StanceTheme theme: host.theme
    signal editMode(string name)
    signal clipRequested
    // Opens the settings window on that category.
    signal settingsRequested(string cat)

    readonly property var entries: service.prefs.board.filter(e => service.widgetShown(e.id))
    readonly property real gap: 10

    implicitHeight: entries.length ? flow.implicitHeight : empty.implicitHeight

    // ---- data the tiles share --------------------------------------------
    readonly property var q: service.quick
    readonly property var night: q ? q.night : null
    readonly property var displays: q ? q.brightness : []
    readonly property var sinks: q ? q.sinks.filter(s => !s.virtual || s.default) : []
    // Brightness tile: the screen the panel opened on, else the first.
    readonly property var display: {
        const sel = host.selected;
        return displays.find(d => sel && d.monitor === sel.name) || displays[0] || null;
    }
    readonly property var sink: sinks.find(s => s.default) || null

    // Whether a widget has something to show at that size. Decided here, not
    // from the item's `visible` (that also turns false with its slot's).
    function available(id, size) {
        switch (id) {
        case "screen": return host.selected !== null;
        case "clip": return host.hasClipShelf;
        case "night": return night !== null;
        case "brightness": return displays.length > 0;
        case "audio": return sinks.length > (size === "S" ? 0 : 1);
        }
        return true;
    }

    function next(list, i) {
        return list.length ? list[(i + 1) % list.length] : null;
    }

    // `stance deck status` while the deck tile is on the board.
    property var deck: null
    property bool deckDown: false
    function refreshDeck() {
        if (!service.widgetShown("deck"))
            return;
        service.call(["deck", "status"], r => {
            board.deckDown = !(r && r.ok);
            board.deck = r && r.ok ? r.data : null;
        });
    }
    Connections {
        target: board.host
        function onVisibleChanged() {
            if (board.host.visible)
                board.refreshDeck();
        }
    }
    Component.onCompleted: refreshDeck()
    // Added to the board while the panel is open.
    onEntriesChanged: if (deck === null && !deckDown) refreshDeck()

    Flow {
        id: flow
        width: board.width
        spacing: board.gap
        visible: board.entries.length > 0

        Repeater {
            model: board.entries
            delegate: Loader {
                id: slot
                required property var modelData
                readonly property string size: modelData.size
                width: size === "S" ? (board.width - board.gap) / 2 : board.width
                // Positioners skip invisible items: an empty widget leaves no gap.
                visible: board.available(modelData.id, size)
                sourceComponent: {
                    const k = modelData.id + "." + size;
                    switch (k) {
                    case "map.M": case "map.L": return mapW;
                    case "screen.L": return screenW;
                    case "night.M": case "brightness.M": case "audio.M": return quickW;
                    case "night.S": return nightTile;
                    case "brightness.S": return brightnessTile;
                    case "audio.S": return audioTile;
                    case "modes.S": return modesTile;
                    case "modes.M": case "modes.L": return modesW;
                    case "clip.S": return clipTile;
                    case "deck.S": return deckTile;
                    }
                    return null;
                }
            }
        }
    }

    // Nothing on the board (all removed, or all off).
    ColumnLayout {
        id: empty
        visible: board.entries.length === 0
        width: board.width
        spacing: 8
        Text {
            textFormat: Text.PlainText
            text: StanceI18n.t("Nothing on the panel yet.")
            color: board.theme.textDim
            font.family: board.theme.fontFamily
            font.pixelSize: board.theme.fontSize - 1
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 8
        }
        StanceChip {
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: 8
            theme: board.theme
            icon: "dashboard_customize"
            label: StanceI18n.t("Choose widgets")
            onClicked: board.settingsRequested("layout")
        }
    }

    // ---- widgets -----------------------------------------------------------
    Component {
        id: mapW
        StanceWidgetMap {
            panel: board.host
            size: parent ? parent.size : "L"
        }
    }
    Component {
        id: screenW
        StanceWidgetScreen {
            panel: board.host
        }
    }
    Component {
        id: quickW
        StanceQuick {
            service: board.service
            theme: board.theme
            part: parent ? parent.modelData.id : ""
        }
    }
    Component {
        id: modesW
        StanceModes {
            service: board.service
            theme: board.theme
            size: parent ? parent.size : "L"
            onEdit: name => board.editMode(name)
        }
    }

    Component {
        id: nightTile
        StanceTile {
            theme: board.theme
            icon: "bedtime"
            title: StanceI18n.t("Night light")
            on: board.night !== null && board.night.on
            value: on ? board.night.temperature + " K" : StanceI18n.t("Off")
            busy: board.service.quickBusy
            onClicked: board.service.runQuick(["night", "toggle"])
        }
    }
    Component {
        id: brightnessTile
        StanceTile {
            theme: board.theme
            icon: board.display && board.display.kind === "backlight" ? "brightness_medium" : "light_mode"
            title: StanceI18n.t("Brightness")
            value: board.display ? board.display.monitor + " · " + Math.round(board.display.percent) + "%" : ""
            tip: StanceI18n.t("Scroll to change")
            busy: board.service.quickBusy
            // Click steps through 25/50/75/100, the wheel by 5.
            onClicked: {
                const p = board.display.percent;
                const n = [25, 50, 75, 100].find(v => v > p + 2) || 25;
                board.service.runQuick(["brightness", board.display.monitor, String(n)]);
            }
            onWheel: step => board.service.runQuick(["brightness", board.display.monitor, (step > 0 ? "+" : "-") + "5"])
        }
    }
    Component {
        id: audioTile
        StanceTile {
            theme: board.theme
            icon: "volume_up"
            title: StanceI18n.t("Audio output")
            value: board.sink ? board.sink.short : ""
            tip: StanceI18n.t("Click for the next output")
            busy: board.service.quickBusy
            onClicked: {
                const n = board.next(board.sinks, board.sinks.indexOf(board.sink));
                if (n && n !== board.sink)
                    board.service.runQuick(["audio", "set", String(n.id)]);
            }
        }
    }
    Component {
        id: modesTile
        StanceTile {
            readonly property var usable: board.service.modes.filter(m => !m.error)
            theme: board.theme
            icon: "bookmarks"
            title: StanceI18n.t("Modes")
            on: board.service.activeMode !== ""
            value: board.service.activeMode || (usable.length ? StanceI18n.t("None active") : StanceI18n.t("No modes yet"))
            tip: StanceI18n.t("Click for the next mode")
            enabled: usable.length > 0 && !board.service.busy
            onClicked: {
                const n = board.next(usable, usable.findIndex(m => m.name === board.service.activeMode));
                if (n)
                    board.service.applyMode(n.name);
            }
        }
    }
    Component {
        id: clipTile
        StanceTile {
            theme: board.theme
            icon: "content_paste"
            title: StanceI18n.t("Clipboard")
            value: StanceI18n.t("Open the shelf")
            onClicked: board.clipRequested()
        }
    }
    Component {
        id: deckTile
        StanceTile {
            theme: board.theme
            icon: "smartphone"
            title: StanceI18n.t("Phone")
            on: board.deck !== null && board.deck.devices.some(d => d.connected)
            value: board.deckDown ? StanceI18n.t("Not running")
                 : board.deck === null ? ""
                 : on ? StanceI18n.t("Connected")
                 : StanceI18n.f("%1 paired", board.deck.devices.length)
            onClicked: board.settingsRequested("phone")
        }
    }
}
