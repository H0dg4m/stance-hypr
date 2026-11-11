import QtQuick
import QtQuick.Layouts

// Modes card: apply a mode, add one, open the editor.
StanceSection {
    id: modesCard
    required property StanceService service
    signal edit(string name)

    property bool adding: false
    property bool withQuick: false
    property string error: ""
    readonly property string newName: nameField.text.trim()
    readonly property bool nameTaken: service.modes.some(m => m.name.toLowerCase() === newName.toLowerCase())
    readonly property bool nameValid: /^[A-Za-z0-9_-]{1,64}$/.test(newName)
    readonly property string nameProblem: newName === "" ? "" : !nameValid ? "Letters, digits, - and _ only" : nameTaken ? "A mode with this name exists" : ""

    icon: "bookmarks"
    title: StanceI18n.t("Modes")
    trailing: [
        StanceChip {
            theme: modesCard.theme
            icon: modesCard.adding ? "close" : "add"
            label: modesCard.adding ? StanceI18n.t("Cancel") : StanceI18n.t("Add")
            onClicked: modesCard.toggleAdd()
        }
    ]

    function toggleAdd() {
        adding = !adding;
        error = "";
        if (adding) {
            nameField.text = "";
            Qt.callLater(() => nameField.focusInput());
        }
    }

    function create() {
        if (!nameValid || nameTaken || service.callBusy)
            return;
        error = "";
        service.saveMode(newName, withQuick, r => {
            if (r && r.ok) {
                const n = newName;
                adding = false;
                edit(n);
            } else {
                error = r && r.error ? r.error.message : StanceI18n.t("stance returned no output");
            }
        });
    }

    // ---- add form ---------------------------------------------------------
    ColumnLayout {
        visible: modesCard.adding
        Layout.fillWidth: true
        spacing: 8
        Text {
            text: StanceI18n.t("Saves the current screens, power profile and workspaces as a new mode.")
            color: modesCard.theme.textDim
            font.family: modesCard.theme.fontFamily
            font.pixelSize: modesCard.theme.fontSize - 2
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        StanceField {
            id: nameField
            Layout.fillWidth: true
            theme: modesCard.theme
            placeholder: StanceI18n.t("Mode name")
            maxLength: 64
            invalid: modesCard.nameProblem !== ""
            validator: RegularExpressionValidator { regularExpression: /[A-Za-z0-9_-]*/ }
            onAccepted: modesCard.create()
            onCancelled: modesCard.toggleAdd()
        }
        Text {
            visible: modesCard.nameProblem !== ""
            text: modesCard.nameProblem
            color: modesCard.theme.danger
            font.family: modesCard.theme.fontFamily
            font.pixelSize: modesCard.theme.fontSize - 2
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            StanceSwitch {
                theme: modesCard.theme
                checked: modesCard.withQuick
                onToggled: v => modesCard.withQuick = v
            }
            Text {
                text: StanceI18n.t("Include night light and audio output")
                color: modesCard.theme.text
                font.family: modesCard.theme.fontFamily
                font.pixelSize: modesCard.theme.fontSize - 1
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
        }
        Text {
            visible: modesCard.error !== ""
            text: modesCard.error
            color: modesCard.theme.danger
            font.family: modesCard.theme.fontFamily
            font.pixelSize: modesCard.theme.fontSize - 1
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        StanceChip {
            Layout.fillWidth: true
            Layout.preferredHeight: 36
            theme: modesCard.theme
            icon: "check"
            label: modesCard.service.callBusy ? StanceI18n.t("Saving…") : StanceI18n.t("Create and edit")
            selected: true
            enabled: modesCard.nameValid && !modesCard.nameTaken && !modesCard.service.callBusy
            onClicked: modesCard.create()
        }
    }

    // ---- list -------------------------------------------------------------
    Flow {
        visible: modesCard.service.modes.length > 0
        Layout.fillWidth: true
        spacing: 6
        Repeater {
            model: modesCard.service.modes
            delegate: Row {
                id: item
                required property var modelData
                spacing: 2
                StanceChip {
                    theme: modesCard.theme
                    icon: item.modelData.error ? "error" : "bookmark"
                    label: item.modelData.name
                    selected: item.modelData.name === modesCard.service.activeMode
                    enabled: !modesCard.service.busy && !item.modelData.error
                    onClicked: modesCard.service.applyMode(item.modelData.name)
                }
                StanceChip {
                    theme: modesCard.theme
                    icon: "edit"
                    tip: StanceI18n.f("Edit %1", item.modelData.name)
                    enabled: !item.modelData.error
                    implicitWidth: 34
                    onClicked: modesCard.edit(item.modelData.name)
                }
            }
        }
    }
    Text {
        visible: modesCard.service.modes.length === 0 && !modesCard.adding
        text: StanceI18n.t("No modes yet. Add one to save the current screens.")
        color: modesCard.theme.textDim
        font.family: modesCard.theme.fontFamily
        font.pixelSize: modesCard.theme.fontSize - 2
        Layout.fillWidth: true
        wrapMode: Text.Wrap
    }
}
