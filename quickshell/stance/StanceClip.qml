import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland

// Clipboard shelf: a keyboard-first overlay with three rows (images, texts,
// links). Keys: arrows move, Up/Down/Tab switch shelf, Space previews, Enter
// copies and closes, P pins, S/Delete removes, Backspace undoes, `/` or typing
// searches, Esc clears the search, then closes. Clip text is always shown as
// plain text. The layer takes exclusive keyboard focus only while it is open.
Scope {
    id: root

    required property StanceClipService clip
    property StanceTheme theme: StanceTheme {}
    property var screen: null
    property bool open: false
    // "Open settings" button of the disabled state.
    signal settingsRequested
    // The "clip" feature switch (stance feature). Off: the shelf shows a short
    // note and a button to turn it on instead of its rows.
    property bool featureOn: true
    signal enableRequested

    onOpenChanged: {
        if (open)
            clip.query = "";
        clip.active = open;
    }

    LazyLoader {
        active: root.open

        component: PanelWindow {
            id: win
            screen: root.screen
            color: "transparent"
            anchors { top: true; bottom: true; left: true; right: true }
            exclusionMode: ExclusionMode.Ignore
            exclusiveZone: 0
            WlrLayershell.namespace: "stance:clip"
            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

            // ---- selection ----------------------------------------------------
            readonly property var shelves: [root.clip.images, root.clip.texts, root.clip.links]
            property int shelf: 0
            property var idx: [0, 0, 0]
            property bool searching: false
            property bool previewing: false
            property bool pauseMenu: false
            property string previewText: ""
            property string curId: ""
            readonly property var current: shelves[shelf][idx[shelf]] || null

            function setIdx(s, i) {
                const a = idx.slice();
                a[s] = i;
                idx = a;
            }

            // After a reload: stay on the same item, else clamp.
            function settle() {
                if (curId !== "") {
                    for (let s = 0; s < 3; s++) {
                        const i = shelves[s].findIndex(x => x.id === curId);
                        if (i >= 0) {
                            shelf = s;
                            setIdx(s, i);
                            return;
                        }
                    }
                }
                const a = idx.map((v, s) => Math.max(0, Math.min(v, shelves[s].length - 1)));
                idx = a;
                if (shelves[shelf].length === 0) {
                    const f = shelves.findIndex(l => l.length > 0);
                    if (f >= 0)
                        shelf = f;
                }
                curId = current ? current.id : "";
            }
            Connections {
                target: root.clip
                function onItemsChanged() { win.settle(); }
            }
            Component.onCompleted: settle()
            onCurrentChanged: if (current) curId = current.id

            function moveH(d) {
                const n = shelves[shelf].length;
                if (n > 0)
                    setIdx(shelf, Math.max(0, Math.min(n - 1, idx[shelf] + d)));
            }
            function moveV(d) {
                for (let k = 1; k <= 3; k++) {
                    const s = (shelf + d * k + 30) % 3;
                    if (shelves[s].length > 0) {
                        shelf = s;
                        return;
                    }
                }
            }
            function doCopy() {
                if (!current)
                    return;
                root.clip.copy(current.id);
                root.open = false;
            }
            function doPin() { if (current) root.clip.pin(current.id, !current.pinned); }
            function doRemove() {
                if (!current)
                    return;
                // Land on the neighbour once the item is gone.
                curId = "";
                root.clip.remove(current.id);
            }
            function setQuery(q) {
                root.clip.query = q;
                queryTimer.restart();
            }
            Timer { id: queryTimer; interval: 120; onTriggered: root.clip.reload() }

            function togglePreview() {
                if (previewing) {
                    previewing = false;
                    return;
                }
                const c = current;
                if (!c)
                    return;
                previewText = "";
                previewing = true;
                if (c.kind !== "image") {
                    previewText = c.preview;
                    const id = c.id;
                    root.clip.fullText(id, t => {
                        if (win.previewing && win.curId === id)
                            win.previewText = t;
                    });
                }
            }

            function printable(e) {
                return e.text.length === 1 && e.text.charCodeAt(0) >= 32 && e.text.charCodeAt(0) !== 127
                    && !(e.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier));
            }

            function handleKey(e) {
                const ctrl = (e.modifiers & Qt.ControlModifier) !== 0;
                e.accepted = true;
                if (previewing) {
                    if (e.key === Qt.Key_Escape || e.key === Qt.Key_Space)
                        previewing = false;
                    else if (e.key === Qt.Key_Return || e.key === Qt.Key_Enter)
                        doCopy();
                    else if (e.key === Qt.Key_Down)
                        previewFlick.contentY = Math.min(Math.max(0, previewFlick.contentHeight - previewFlick.height), previewFlick.contentY + 60);
                    else if (e.key === Qt.Key_Up)
                        previewFlick.contentY = Math.max(0, previewFlick.contentY - 60);
                    return;
                }
                switch (e.key) {
                case Qt.Key_Escape:
                    if (pauseMenu) {
                        pauseMenu = false;
                    } else if (root.clip.query !== "") {
                        searching = false;
                        setQuery("");
                    } else {
                        root.open = false;
                    }
                    return;
                case Qt.Key_Return:
                case Qt.Key_Enter: doCopy(); return;
                case Qt.Key_Left: moveH(-1); return;
                case Qt.Key_Right: moveH(1); return;
                case Qt.Key_Up: searching = false; moveV(-1); return;
                case Qt.Key_Down: searching = false; moveV(1); return;
                case Qt.Key_Tab: searching = false; moveV(1); return;
                case Qt.Key_Backtab: searching = false; moveV(-1); return;
                case Qt.Key_Delete: doRemove(); return;
                }
                if (ctrl && e.key === Qt.Key_P) { doPin(); return; }
                if (ctrl && e.key === Qt.Key_S) { doRemove(); return; }
                if (e.key === Qt.Key_Backspace) {
                    if (root.clip.query !== "") {
                        searching = true;
                        setQuery(root.clip.query.slice(0, -1));
                    } else {
                        root.clip.undo();
                    }
                    return;
                }
                if (!searching) {
                    if (e.key === Qt.Key_Space) { togglePreview(); return; }
                    if (e.key === Qt.Key_P) { doPin(); return; }
                    if (e.key === Qt.Key_S) { doRemove(); return; }
                    if (e.key === Qt.Key_Slash) { searching = true; return; }
                }
                if (printable(e)) {
                    searching = true;
                    setQuery(root.clip.query + e.text);
                    return;
                }
                e.accepted = false;
            }

            // ---- helpers ------------------------------------------------------
            function ago(t) {
                const s = Math.max(0, Date.now() / 1000 - t);
                if (s < 60) return StanceI18n.t("now");
                if (s < 3600) return StanceI18n.f("%1 min", Math.floor(s / 60));
                if (s < 86400) return StanceI18n.f("%1 h", Math.floor(s / 3600));
                return StanceI18n.f("%1 d", Math.floor(s / 86400));
            }
            // "HH:mm", with the weekday when it is not today.
            function pauseText(secs) {
                const d = new Date(secs * 1000);
                return d.toDateString() === new Date().toDateString() ? Qt.formatTime(d, "HH:mm") : Qt.formatDateTime(d, "ddd HH:mm");
            }
            function bytes(n) {
                if (n < 1024) return n + " B";
                if (n < 1048576) return Math.round(n / 1024) + " KB";
                return (n / 1048576).toFixed(1) + " MB";
            }

            // ---- view ---------------------------------------------------------
            Item {
                id: keys
                anchors.fill: parent
                focus: true
                Keys.onPressed: e => win.handleKey(e)

                Rectangle {
                    anchors.fill: parent
                    color: root.theme.scrim
                    MouseArea { anchors.fill: parent; onClicked: root.open = false }
                }

                Rectangle {
                    id: card
                    anchors.centerIn: parent
                    width: Math.min(win.width - 48, 1080)
                    height: Math.min(win.height - 48, 620)
                    radius: root.theme.radius
                    // Overlay: always opaque, whatever the theme's opacity.
                    color: root.theme.backgroundSolid
                    border.width: 1
                    border.color: root.theme.border
                    MouseArea { anchors.fill: parent }

                    opacity: 0
                    Component.onCompleted: fadeIn.start()
                    NumberAnimation { id: fadeIn; target: card; property: "opacity"; to: 1; duration: root.theme.animMs * 1.5; easing.type: Easing.OutCubic }

                    // Search line
                    Rectangle {
                        id: searchBox
                        x: 16
                        y: 16
                        width: parent.width - 32
                        height: 36
                        radius: root.theme.chipRadius
                        color: root.theme.surface
                        border.width: 2
                        border.color: win.searching ? root.theme.accent : "transparent"
                        Row {
                            anchors.verticalCenter: parent.verticalCenter
                            x: 12
                            spacing: 8
                            StanceIcon { theme: root.theme; name: "search"; color: root.theme.textDim; visible: root.theme.iconFont !== "" }
                            Text {
                                text: root.clip.query !== "" ? root.clip.query : StanceI18n.t("Type to search, or press /")
                                textFormat: Text.PlainText
                                color: root.clip.query !== "" ? root.theme.text : root.theme.textDim
                                font.family: root.theme.fontFamily
                                font.pixelSize: root.theme.fontSize + 1
                            }
                            Rectangle {
                                visible: win.searching
                                width: 2
                                height: root.theme.fontSize + 2
                                color: root.theme.accent
                                anchors.verticalCenter: parent.verticalCenter
                            }
                        }
                        StanceChip {
                            id: closeChip
                            theme: root.theme
                            icon: "close"
                            label: root.theme.iconFont === "" ? StanceI18n.t("Close") : ""
                            anchors.right: parent.right
                            anchors.rightMargin: 3
                            anchors.verticalCenter: parent.verticalCenter
                            implicitHeight: 30
                            onClicked: root.open = false
                        }
                        // Capture pause: a Pause menu, or the state + Resume.
                        Row {
                            visible: root.featureOn && root.clip.capture
                            anchors.right: closeChip.left
                            anchors.rightMargin: 6
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: 6
                            StanceChip {
                                visible: root.clip.paused
                                theme: root.theme
                                icon: "pause_circle"
                                label: StanceI18n.f("Paused until %1", win.pauseText(root.clip.pausedUntil))
                                selected: true
                                implicitHeight: 30
                            }
                            StanceChip {
                                visible: root.clip.paused
                                theme: root.theme
                                icon: "play_arrow"
                                label: StanceI18n.t("Resume")
                                implicitHeight: 30
                                onClicked: root.clip.resume()
                            }
                            StanceChip {
                                id: pauseChip
                                visible: !root.clip.paused
                                theme: root.theme
                                icon: "pause"
                                label: StanceI18n.t("Pause")
                                selected: win.pauseMenu
                                implicitHeight: 30
                                onClicked: win.pauseMenu = !win.pauseMenu
                            }
                        }
                    }

                    // Pause menu: how long capture stays off.
                    Rectangle {
                        id: pauseMenuBox
                        visible: win.pauseMenu && !root.clip.paused
                        z: 20
                        width: 170
                        height: pmCol.implicitHeight + 12
                        x: searchBox.x + searchBox.width - closeChip.width - 9 - width
                        y: searchBox.y + searchBox.height + 6
                        radius: root.theme.chipRadius + 2
                        color: root.theme.card
                        border.width: 1
                        border.color: root.theme.border
                        Column {
                            id: pmCol
                            x: 6
                            y: 6
                            width: parent.width - 12
                            spacing: 2
                            Repeater {
                                model: [
                                    { label: StanceI18n.t("15 min"), value: "15m" },
                                    { label: StanceI18n.t("1 hour"), value: "1h" },
                                    { label: StanceI18n.t("Until resumed"), value: "24h" }
                                ]
                                delegate: Rectangle {
                                    required property var modelData
                                    width: pmCol.width
                                    height: 32
                                    radius: root.theme.chipRadius
                                    color: pma.containsMouse ? root.theme.surfaceHover : "transparent"
                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        x: 10
                                        text: modelData.label
                                        color: root.theme.text
                                        font.family: root.theme.fontFamily
                                        font.pixelSize: root.theme.fontSize
                                    }
                                    MouseArea {
                                        id: pma
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: {
                                            win.pauseMenu = false;
                                            root.clip.pause(modelData.value);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Empty / disabled / error states
                    Column {
                        id: stateBox
                        visible: !root.featureOn || root.clip.items.length === 0
                        anchors.centerIn: parent
                        spacing: 12
                        Text {
                            anchors.horizontalCenter: parent.horizontalCenter
                            textFormat: Text.PlainText
                            horizontalAlignment: Text.AlignHCenter
                            color: root.theme.textDim
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize + 2
                            text: !root.featureOn ? StanceI18n.t("Clipboard shelf is turned off")
                                : root.clip.error !== "" ? root.clip.error
                                : !root.clip.loaded ? StanceI18n.t("Loading…")
                                : !root.clip.capture ? StanceI18n.t("Clipboard shelf is off — enable in Settings")
                                : root.clip.query !== "" ? StanceI18n.f("Nothing matches “%1”", root.clip.query)
                                : StanceI18n.t("Nothing copied yet")
                        }
                        StanceChip {
                            visible: !root.featureOn
                            anchors.horizontalCenter: parent.horizontalCenter
                            theme: root.theme
                            icon: "power_settings_new"
                            label: StanceI18n.t("Turn on")
                            selected: true
                            onClicked: root.enableRequested()
                        }
                        StanceChip {
                            visible: root.featureOn && root.clip.loaded && !root.clip.capture && root.clip.error === ""
                            anchors.horizontalCenter: parent.horizontalCenter
                            theme: root.theme
                            icon: "settings"
                            label: StanceI18n.t("Open settings")
                            onClicked: {
                                root.open = false;
                                root.settingsRequested();
                            }
                        }
                    }

                    // Three shelves
                    Column {
                        visible: root.featureOn && root.clip.items.length > 0 && !win.previewing
                        x: 16
                        y: searchBox.y + searchBox.height + 12
                        width: parent.width - 32
                        spacing: 10

                        Repeater {
                          model: [
                            { title: StanceI18n.t("Images"), which: 0, cardW: 180, cardH: 120 },
                            { title: StanceI18n.t("Texts"), which: 1, cardW: 240, cardH: 96 },
                            { title: StanceI18n.t("Links"), which: 2, cardW: 240, cardH: 68 }
                          ]
                          delegate: Item {
                            id: shelfRoot
                            required property var modelData
                            readonly property string title: modelData.title
                            readonly property var list: win.shelves[modelData.which]
                            readonly property int which: modelData.which
                            readonly property real cardW: modelData.cardW
                            readonly property real cardH: modelData.cardH
                            width: parent.width
                            height: 22 + (list.length > 0 ? cardH + 8 : 26)

                            readonly property bool active: win.shelf === which
                            readonly property int hi: win.idx[which]
                            onHiChanged: if (active && list.length > 0) view.positionViewAtIndex(hi, ListView.Contain)
                            onActiveChanged: if (active && list.length > 0) view.positionViewAtIndex(hi, ListView.Contain)

                            Row {
                                spacing: 8
                                height: 22
                                Text {
                                    text: shelfRoot.title
                                    color: shelfRoot.active ? root.theme.accent : root.theme.textDim
                                    font.family: root.theme.fontFamily
                                    font.pixelSize: root.theme.fontSize - 1
                                    font.weight: Font.DemiBold
                                    anchors.verticalCenter: parent.verticalCenter
                                }
                                Text {
                                    text: shelfRoot.list.length
                                    color: root.theme.textDim
                                    font.family: root.theme.fontFamily
                                    font.pixelSize: root.theme.fontSize - 2
                                    anchors.verticalCenter: parent.verticalCenter
                                }
                            }
                            Text {
                                visible: shelfRoot.list.length === 0
                                y: 22
                                text: StanceI18n.t("empty")
                                color: root.theme.textDim
                                opacity: 0.7
                                font.family: root.theme.fontFamily
                                font.pixelSize: root.theme.fontSize - 1
                            }
                            ListView {
                                id: view
                                visible: shelfRoot.list.length > 0
                                y: 22
                                width: parent.width
                                height: shelfRoot.cardH + 8
                                orientation: ListView.Horizontal
                                spacing: 10
                                clip: true
                                model: shelfRoot.list
                                boundsBehavior: Flickable.StopAtBounds
                                delegate: Item {
                                    id: cell
                                    required property var modelData
                                    required property int index
                                    readonly property bool sel: shelfRoot.active && shelfRoot.hi === index
                                    width: shelfRoot.cardW
                                    height: shelfRoot.cardH + 8
                                    Rectangle {
                                        anchors.fill: parent
                                        anchors.margins: 3
                                        radius: root.theme.chipRadius
                                        color: cell.sel ? root.theme.surfaceHover : root.theme.card
                                        clip: true

                                        Loader {
                                            anchors.fill: parent
                                            sourceComponent: cell.modelData.kind === "image" ? imageFace : (cell.modelData.kind === "link" ? linkFace : textFace)
                                            property var item: cell.modelData
                                        }
                                        // Border on top so images do not cover it.
                                        Rectangle {
                                            anchors.fill: parent
                                            radius: parent.radius
                                            color: "transparent"
                                            border.width: cell.sel ? 2 : 1
                                            border.color: cell.sel ? root.theme.accent : root.theme.border
                                        }
                                        // Pin badge
                                        Rectangle {
                                            visible: cell.modelData.pinned
                                            anchors.top: parent.top
                                            anchors.right: parent.right
                                            anchors.margins: 6
                                            radius: 4
                                            color: root.theme.accent
                                            width: pinLabel.implicitWidth + 10
                                            height: 16
                                            Text {
                                                id: pinLabel
                                                anchors.centerIn: parent
                                                text: StanceI18n.t("pinned")
                                                color: root.theme.accentText
                                                font.family: root.theme.fontFamily
                                                font.pixelSize: root.theme.fontSize - 4
                                                font.weight: Font.DemiBold
                                            }
                                        }
                                        MouseArea {
                                            anchors.fill: parent
                                            onClicked: {
                                                win.shelf = shelfRoot.which;
                                                win.setIdx(shelfRoot.which, cell.index);
                                                win.doCopy();
                                            }
                                        }
                                    }
                                }
                            }
                        }

                          }
                    }

                    // Footer: key hints, then notes / errors
                    Item {
                        id: footer
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.margins: 16
                        height: 20
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: StanceI18n.t("←→ move   ↑↓ shelf   Space preview   Enter copy   P pin   S remove   ⌫ undo   / search   Esc close")
                            color: root.theme.textDim
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize - 2
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.right: parent.right
                            textFormat: Text.PlainText
                            text: !root.featureOn ? ""
                                : root.clip.error !== "" && root.clip.items.length > 0 ? root.clip.error
                                : (!root.clip.capture && root.clip.loaded ? StanceI18n.t("capture is off") : root.clip.note)
                            color: root.clip.error !== "" ? root.theme.danger : root.theme.accent
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize - 2
                        }
                    }

                    // Larger preview of the selected item
                    Rectangle {
                        id: previewBox
                        visible: root.featureOn && win.previewing && win.current !== null
                        // Opaque and below the search line: nothing of the
                        // shelves (hidden meanwhile) can show through.
                        anchors.fill: parent
                        anchors.margins: 12
                        anchors.topMargin: searchBox.y + searchBox.height + 12
                        anchors.bottomMargin: 40
                        radius: root.theme.chipRadius
                        color: root.theme.cardSolid
                        border.width: 1
                        border.color: root.theme.border
                        MouseArea { anchors.fill: parent; onClicked: win.previewing = false }

                        Image {
                            visible: win.current !== null && win.current.kind === "image"
                            anchors.fill: parent
                            anchors.margins: 10
                            fillMode: Image.PreserveAspectFit
                            asynchronous: true
                            cache: false
                            sourceSize: Qt.size(1600, 1600)
                            source: win.previewing && win.current && win.current.kind === "image" && win.current.path !== "" ? "file://" + win.current.path.split("/").map(encodeURIComponent).join("/") : ""
                        }
                        Flickable {
                            id: previewFlick
                            visible: win.current !== null && win.current.kind !== "image"
                            anchors.fill: parent
                            anchors.margins: 12
                            clip: true
                            contentWidth: width
                            contentHeight: previewArea.implicitHeight
                            boundsBehavior: Flickable.StopAtBounds
                            ScrollBar.vertical: ScrollBar {}
                            TextArea {
                                id: previewArea
                                width: previewFlick.width - 12
                                readOnly: true
                                activeFocusOnPress: false
                                focus: false
                                textFormat: TextEdit.PlainText
                                wrapMode: TextEdit.Wrap
                                text: win.previewText
                                color: root.theme.text
                                selectionColor: root.theme.accent
                                selectedTextColor: root.theme.accentText
                                background: null
                                font.family: root.theme.fontFamily
                                font.pixelSize: root.theme.fontSize + 1
                            }
                        }
                    }
                }
            }

            // ---- card faces ------------------------------------------------------
            Component {
                id: imageFace
                Item {
                    Text {
                        anchors.centerIn: parent
                        visible: item.path === ""
                        text: StanceI18n.t("no preview")
                        color: root.theme.textDim
                        font.family: root.theme.fontFamily
                        font.pixelSize: root.theme.fontSize - 1
                    }
                    Image {
                        anchors.fill: parent
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: false
                        sourceSize: Qt.size(320, 200)
                        source: item.path !== "" ? "file://" + item.path.split("/").map(encodeURIComponent).join("/") : ""
                    }
                    Rectangle {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        height: 20
                        color: "#99000000"
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            x: 8
                            text: (item.w > 0 ? item.w + "×" + item.h + " · " : "") + win.bytes(item.size) + " · " + win.ago(item.updated)
                            color: "#ffffff"
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize - 3
                        }
                    }
                }
            }
            Component {
                id: textFace
                Item {
                    Text {
                        x: 10
                        y: 8
                        width: parent.width - 20
                        height: parent.height - 30
                        textFormat: Text.PlainText
                        text: item.preview
                        wrapMode: Text.Wrap
                        maximumLineCount: 4
                        elide: Text.ElideRight
                        color: root.theme.text
                        font.family: root.theme.fontFamily
                        font.pixelSize: root.theme.fontSize - 1
                    }
                    Text {
                        anchors.left: parent.left
                        anchors.bottom: parent.bottom
                        anchors.margins: 8
                        text: win.bytes(item.size) + " · " + win.ago(item.updated)
                        color: root.theme.textDim
                        font.family: root.theme.fontFamily
                        font.pixelSize: root.theme.fontSize - 3
                    }
                }
            }
            Component {
                id: linkFace
                Item {
                    Column {
                        x: 10
                        y: 8
                        width: parent.width - 20
                        spacing: 3
                        Text {
                            width: parent.width
                            textFormat: Text.PlainText
                            text: item.host
                            elide: Text.ElideRight
                            color: root.theme.accent
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize
                            font.weight: Font.DemiBold
                        }
                        Text {
                            width: parent.width
                            textFormat: Text.PlainText
                            text: item.preview
                            elide: Text.ElideMiddle
                            color: root.theme.text
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize - 2
                        }
                        Text {
                            text: win.ago(item.updated)
                            color: root.theme.textDim
                            font.family: root.theme.fontFamily
                            font.pixelSize: root.theme.fontSize - 3
                        }
                    }
                }
            }
        }
    }
}
