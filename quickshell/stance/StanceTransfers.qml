import QtQuick
import QtQuick.Layouts

// Settings → Phone: clipboard and file transfers with paired phones
// (`stance deck transfer`, transfer.toml; docs/deck.md). Everything is off
// until switched on here. Also shows a file offer waiting for an answer.
StanceSection {
    id: card

    required property StanceService service
    property var settings: null
    property var pending: null
    property var recent: []
    property var devices: []
    property string error: ""

    icon: "swap_horiz"
    title: StanceI18n.t("Clipboard and files")

    function load() {
        service.call(["deck", "transfer", "show"], r => {
            card.settings = r && r.ok ? r.data.settings : null;
            card.error = r && r.ok ? (r.data.warning || "") : (r && r.error ? r.error.message : "");
        });
        service.call(["deck", "transfers"], r => {
            card.pending = r && r.ok ? r.data.pending : null;
            card.recent = r && r.ok && r.data.transfers ? r.data.transfers.slice(-3).reverse() : [];
        });
    }

    function put(key, value) {
        if (!settings)
            return;
        const s = JSON.parse(JSON.stringify(settings));
        delete s.schema;
        s[key] = value;
        service.call(["deck", "transfer", "put", "--data", JSON.stringify(s)], r => {
            if (r && r.ok)
                card.settings = r.data.settings;
            else
                card.error = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
        });
    }

    function trust(name, on) {
        if (!settings)
            return;
        const list = (settings.trusted_devices || []).filter(n => n !== name);
        if (on)
            list.push(name);
        put("trusted_devices", list);
    }

    function answer(yes) {
        if (!pending)
            return;
        service.call(["deck", "transfer", yes ? "accept" : "decline", String(pending.id)], () => card.load());
    }

    Component.onCompleted: {
        load();
        // Paired phones change rarely; once per opening is enough.
        service.deckDevices(r => {
            card.devices = r && r.ok && r.data.devices ? r.data.devices.map(d => d.name) : [];
        });
    }
    // A prompt can arrive while the page is open.
    Timer {
        interval: 2000
        repeat: true
        running: card.visible
        onTriggered: card.load()
    }

    component Row_: RowLayout {
        property alias text: label.text
        property alias hint: hintText.text
        default property alias control: slot.data
        Layout.fillWidth: true
        spacing: 10
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 1
            Text {
                id: label
                textFormat: Text.PlainText
                color: card.theme.text
                font.family: card.theme.fontFamily
                font.pixelSize: card.theme.fontSize
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Text {
                id: hintText
                visible: text !== ""
                textFormat: Text.PlainText
                color: card.theme.textDim
                font.family: card.theme.fontFamily
                font.pixelSize: card.theme.fontSize - 2
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
        Item {
            id: slot
            implicitWidth: childrenRect.width
            implicitHeight: childrenRect.height
        }
    }

    // ---- a file waiting for an answer ---------------------------------------
    Rectangle {
        visible: card.pending !== null
        Layout.fillWidth: true
        radius: card.theme.chipRadius
        color: card.theme.accentSoft
        implicitHeight: pend.implicitHeight + 16
        RowLayout {
            id: pend
            x: 10; y: 8
            width: parent.width - 20
            spacing: 8
            Text {
                text: card.pending ? StanceI18n.f("%1 wants to send %2", card.pending.device, card.pending.name) : ""
                textFormat: Text.PlainText
                color: card.theme.text
                font.family: card.theme.fontFamily
                font.pixelSize: card.theme.fontSize - 1
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            StanceChip { theme: card.theme; label: StanceI18n.t("Decline"); onClicked: card.answer(false) }
            StanceChip { theme: card.theme; label: StanceI18n.t("Accept"); selected: true; onClicked: card.answer(true) }
        }
    }

    Row_ {
        text: StanceI18n.t("Send what I copy to the phone")
        hint: StanceI18n.t("New copies from the clipboard shelf, while the phone app is open. Needs the shelf on.")
        StanceSwitch {
            theme: card.theme
            enabled: card.settings !== null
            checked: card.settings !== null && card.settings.clip_to_phone
            onToggled: v => card.put("clip_to_phone", v)
        }
    }
    Row_ {
        text: StanceI18n.t("Take clipboard text from the phone")
        hint: StanceI18n.t("Replaces the PC clipboard when you press Send on the phone; a notification shows the text.")
        StanceSwitch {
            theme: card.theme
            enabled: card.settings !== null
            checked: card.settings !== null && card.settings.clip_from_phone === "notify"
            onToggled: v => card.put("clip_from_phone", v ? "notify" : "off")
        }
    }
    Row_ {
        text: StanceI18n.t("Images too")
        hint: StanceI18n.t("Clipboard images (PNG, JPEG, WebP, GIF) both ways, as the switches above allow.")
        StanceSwitch {
            theme: card.theme
            enabled: card.settings !== null && (card.settings.clip_to_phone || card.settings.clip_from_phone === "notify")
            checked: card.settings !== null && card.settings.clip_images
            onToggled: v => card.put("clip_images", v)
        }
    }
    Row_ {
        text: StanceI18n.t("Take files from the phone")
        hint: card.settings ? StanceI18n.f("Each file asks first. Saved in %1.", card.settings.download_dir || "~/Downloads/stance") : ""
        StanceSwitch {
            theme: card.theme
            enabled: card.settings !== null
            checked: card.settings !== null && card.settings.files_from_phone === "ask"
            onToggled: v => card.put("files_from_phone", v ? "ask" : "off")
        }
    }
    Repeater {
        model: card.settings && card.settings.files_from_phone === "ask" ? card.devices : []
        delegate: Row_ {
            required property var modelData
            text: StanceI18n.f("Save files from %1 without asking", modelData)
            hint: StanceI18n.t("Still announced when saved. Only for your own phones.")
            StanceSwitch {
                theme: card.theme
                checked: card.settings !== null && (card.settings.trusted_devices || []).indexOf(modelData) >= 0
                onToggled: v => card.trust(modelData, v)
            }
        }
    }
    Text {
        visible: card.recent.length > 0
        text: card.recent.map(t => (t.direction === "to_phone" ? "→ " : "← ") + t.name + " · " + StanceI18n.t(t.state)).join("\n")
        textFormat: Text.PlainText
        color: card.theme.textDim
        font.family: card.theme.fontFamily
        font.pixelSize: card.theme.fontSize - 2
        elide: Text.ElideRight
        Layout.fillWidth: true
    }
    Text {
        visible: card.recent.length === 0 && card.settings !== null
        text: StanceI18n.t("Send a file from here with: stance deck send FILE")
        textFormat: Text.PlainText
        color: card.theme.textDim
        font.family: card.theme.fontFamily
        font.pixelSize: card.theme.fontSize - 2
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Text {
        visible: card.error !== ""
        text: card.error
        textFormat: Text.PlainText
        color: card.theme.danger
        font.family: card.theme.fontFamily
        font.pixelSize: card.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
