import QtQuick
import QtQuick.Layouts

// Settings page: pair and manage phone deck devices.
ColumnLayout {
    id: page
    required property StanceService service
    required property StanceTheme theme
    readonly property var prefs: service.prefs

    Layout.fillWidth: true
    spacing: 10

    // ---- phone deck state -----------------------------------------------------
    property var deckInfo: null        // data of `deck devices`, null until loaded
    property var pair: null            // {payload, svg, fingerprint, until, baseline} while the QR shows
    property real pairNow: Date.now()
    property string pairError: ""
    property string unpairAsk: ""      // device waiting for the second click
    readonly property var deckServer: deckInfo ? deckInfo.server : null
    readonly property var deckList: deckServer ? deckServer.devices : (deckInfo ? deckInfo.devices : [])
    readonly property int pairLeft: pair ? Math.max(0, Math.ceil((pair.until - pairNow) / 1000)) : 0

    function loadDeck(after) {
        service.deckDevices(r => {
            if (r && r.ok)
                deckInfo = r.data;
            if (after)
                after();
        });
    }
    function hidePair() {
        pair = null;
        pairError = "";
    }
    function startPair() {
        pairError = "";
        service.deckPair(r => {
            if (r && r.ok) {
                const n = deckList.length;
                pairNow = Date.now();
                pair = { payload: r.data.payload, svg: r.data.svg, fingerprint: r.data.fingerprint, until: Date.now() + r.data.expires_ms, baseline: n };
            } else {
                pairError = r && r.error ? (r.error.code === "deck_not_running" ? "The deck server is not running." : r.error.message) : StanceI18n.t("stance returned no output");
            }
        });
    }
    function unpair(name) {
        if (unpairAsk !== name) {
            unpairAsk = name;
            return;
        }
        unpairAsk = "";
        service.deckUnpair(name, r => {
            if (r && !r.ok)
                pairError = r.error ? r.error.message : StanceI18n.t("unpair failed");
            loadDeck();
        });
    }
    // Countdown, and polling for a new device while the QR shows.
    Timer {
        interval: 1000; repeat: true; running: page.pair !== null
        onTriggered: {
            page.pairNow = Date.now();
            if (page.pairLeft <= 0)
                page.hidePair();
        }
    }
    Timer {
        interval: 2000; repeat: true; running: page.pair !== null
        onTriggered: page.loadDeck(() => {
            if (page.pair && page.deckList.length > page.pair.baseline)
                page.hidePair();
        })
    }

    readonly property var deckCheck: checks.find(c => c.id === "deck" && c.status === "warn")
    property var checks: []
    function loadChecks() {
        service.doctor(r => {
            if (r && r.ok)
                checks = r.data;
        });
    }

    Component.onCompleted: {
        loadDeck();
        loadChecks();
    }

    component Dim: Text {
        textFormat: Text.PlainText
        color: page.theme.textDim
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize - 1
    }

    component PageTitle: Text {
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize + 2
        font.weight: Font.DemiBold
        Layout.fillWidth: true
        Layout.bottomMargin: 2
    }

    component Body: Text {
        textFormat: Text.PlainText
        color: page.theme.text
        font.family: page.theme.fontFamily
        font.pixelSize: page.theme.fontSize
    }

    PageTitle { text: StanceI18n.t("Phone") }
    StanceSection {
        Layout.fillWidth: true
        theme: page.theme
        icon: "smartphone"
        visible: page.service.featureOn("deck")
        title: StanceI18n.t("Phone deck")
        trailing: [
            StanceChip {
                theme: page.theme
                icon: page.pair ? "close" : "qr_code_2"
                label: page.pair ? StanceI18n.t("Hide") : StanceI18n.t("Pair a phone")
                enabled: page.deckServer !== null && !page.service.callBusy
                onClicked: page.pair ? page.hidePair() : page.startPair()
            }
        ]

        Dim {
            visible: page.deckInfo === null
            text: StanceI18n.t("Checking…")
        }
        Dim {
            visible: page.deckServer !== null
            text: page.deckServer ? StanceI18n.f(page.deckServer.loopback ? "Running on port %1 (this PC only)" : "Running on port %1", page.deckServer.port) + (page.deckServer.hosts.length ? " · " + page.deckServer.hosts.join(", ") : "") : ""
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            font.pixelSize: page.theme.fontSize - 2
        }
        TextEdit {
            textFormat: TextEdit.PlainText
            visible: page.deckInfo !== null && page.deckServer === null
            readOnly: true
            selectByMouse: true
            text: StanceI18n.t("Phone deck is off. Start it: systemctl --user enable --now stance-deck.service (or run `stance deck serve`)")
            wrapMode: TextEdit.Wrap
            Layout.fillWidth: true
            color: page.theme.textDim
            selectionColor: page.theme.accent
            selectedTextColor: page.theme.accentText
            font.family: page.theme.fontFamily
            font.pixelSize: page.theme.fontSize - 2
        }
        RowLayout {
            visible: page.deckCheck !== undefined && page.deckCheck !== null
            Layout.fillWidth: true
            spacing: 8
            StanceIcon { theme: page.theme; name: "warning"; size: page.theme.fontSize + 3; color: page.theme.textDim; Layout.alignment: Qt.AlignTop }
            TextEdit {
                textFormat: TextEdit.PlainText
                readOnly: true
                selectByMouse: true
                text: page.deckCheck ? page.deckCheck.message : ""
                wrapMode: TextEdit.Wrap
                Layout.fillWidth: true
                color: page.theme.textDim
                selectionColor: page.theme.accent
                selectedTextColor: page.theme.accentText
                font.family: page.theme.fontFamily
                font.pixelSize: page.theme.fontSize - 2
            }
        }

        // QR while the pairing window is open
        ColumnLayout {
            visible: page.pair !== null
            Layout.fillWidth: true
            spacing: 6
            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                implicitWidth: 236
                implicitHeight: 236
                radius: 8
                color: "white"
                Image {
                    anchors.centerIn: parent
                    width: 220
                    height: 220
                    source: page.pair ? "file://" + page.pair.svg.split("/").map(encodeURIComponent).join("/") : ""
                    cache: false
                    sourceSize: Qt.size(220, 220)
                    fillMode: Image.PreserveAspectFit
                }
            }
            Body {
                Layout.alignment: Qt.AlignHCenter
                text: StanceI18n.f("Scan within %1 s", page.pairLeft)
            }
            Dim {
                Layout.alignment: Qt.AlignHCenter
                text: page.pair ? StanceI18n.f("This PC's key: %1", page.pair.fingerprint) : ""
                font.pixelSize: page.theme.fontSize - 2
            }
            TextEdit {
                textFormat: TextEdit.PlainText
                readOnly: true
                selectByMouse: true
                text: page.pair ? page.pair.payload : ""
                wrapMode: TextEdit.WrapAnywhere
                Layout.fillWidth: true
                color: page.theme.textDim
                selectionColor: page.theme.accent
                selectedTextColor: page.theme.accentText
                font.family: "monospace"
                font.pixelSize: page.theme.fontSize - 5
            }
            // The panel closes when focus moves to a terminal, so typing the
            // code by hand is not an option: copy a ready command instead.
            // (Single use and 120 s, so a clipboard history copy is harmless.)
            StanceChip {
                Layout.alignment: Qt.AlignHCenter
                theme: page.theme
                icon: copied ? "check" : "content_copy"
                label: copied ? StanceI18n.t("Copied — paste in a terminal") : StanceI18n.t("Copy test command")
                property bool copied: false
                onClicked: {
                    Quickshell.clipboardText = "stance deck test pair '" + page.pair.payload + "'";
                    copied = true;
                }
            }
            Dim {
                textFormat: Text.PlainText
                text: StanceI18n.t("Or give this code to `stance deck test pair`. Anyone who sees it now can pair, so it disappears after use.")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                font.pixelSize: page.theme.fontSize - 2
            }
        }

        Repeater {
            model: page.deckList
            delegate: Rectangle {
                id: drow
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: dr.implicitHeight + 16
                radius: page.theme.chipRadius
                color: page.theme.surface
                RowLayout {
                    id: dr
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 8
                    Rectangle {
                        implicitWidth: 8
                        implicitHeight: 8
                        radius: 4
                        visible: drow.modelData.connected !== undefined
                        color: drow.modelData.connected ? page.theme.accent : "transparent"
                        border.width: 1
                        border.color: drow.modelData.connected ? page.theme.accent : page.theme.textDim
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Body { text: drow.modelData.name; elide: Text.ElideRight; Layout.fillWidth: true }
                        Dim {
                            text: drow.modelData.connected === true ? StanceI18n.t("connected") : StanceI18n.t("not connected")
                            visible: drow.modelData.connected !== undefined
                            font.pixelSize: page.theme.fontSize - 3
                        }
                    }
                    StanceChip {
                        theme: page.theme
                        icon: "link_off"
                        label: page.unpairAsk === drow.modelData.name ? StanceI18n.t("Sure?") : StanceI18n.t("Unpair")
                        danger: true
                        enabled: !page.service.callBusy
                        onClicked: page.unpair(drow.modelData.name)
                    }
                }
            }
        }
        Dim {
            visible: page.deckInfo !== null && page.deckList.length === 0
            text: StanceI18n.t("No phones paired.")
            font.pixelSize: page.theme.fontSize - 2
        }
        Text {
            textFormat: Text.PlainText
            visible: page.pairError !== ""
            text: page.pairError
            color: page.theme.danger
            font.family: page.theme.fontFamily
            font.pixelSize: page.theme.fontSize - 1
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }

    StanceTransfers {
        visible: page.service.featureOn("deck")
        Layout.fillWidth: true
        theme: page.theme
        service: page.service
    }

    StanceDeckButtons {
        visible: page.service.featureOn("deck")
        Layout.fillWidth: true
        theme: page.theme
        service: page.service
    }
}
