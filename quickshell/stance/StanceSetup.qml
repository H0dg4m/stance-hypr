import QtQuick
import QtQuick.Layouts

// First-run card on top of the board (shown while panel.json says
// `onboarded: false`, i.e. there was no panel.json). A preset changes the
// board below it right away, so the board is the preview. Closing the
// panel loses nothing: the card comes back until Done.
StanceSection {
    id: setup

    required property StanceService service
    signal customize

    icon: "waving_hand"
    title: StanceI18n.t("Welcome to stance")

    // The preset the current board equals, or "" after other changes.
    readonly property string current: {
        const b = JSON.stringify(service.prefs.board);
        return StanceWidgets.presetNames.find(n => JSON.stringify(StanceWidgets.normBoard(StanceWidgets.preset(n))) === b) || "";
    }

    Text {
        text: StanceI18n.t("This is your panel. Pick what it shows; sizes and order can be changed any time in Settings → Layout.")
        color: setup.theme.text
        font.family: setup.theme.fontFamily
        font.pixelSize: setup.theme.fontSize - 1
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    StanceSegmented {
        Layout.fillWidth: true
        theme: setup.theme
        model: [
            { label: StanceI18n.t("Minimal"), value: "minimal" },
            { label: StanceI18n.t("Standard"), value: "standard" },
            { label: StanceI18n.t("Everything"), value: "everything" }
        ]
        current: setup.current
        onPicked: v => setup.service.setBoard(StanceWidgets.preset(v))
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        StanceChip {
            theme: setup.theme
            icon: "dashboard_customize"
            label: StanceI18n.t("Customize")
            onClicked: {
                setup.service.setPref("settingsPage", "layout");
                setup.customize();
            }
        }
        Item { Layout.fillWidth: true }
        StanceChip {
            theme: setup.theme
            icon: "check"
            label: StanceI18n.t("Done")
            selected: true
            onClicked: setup.service.finishSetup()
        }
    }
}
