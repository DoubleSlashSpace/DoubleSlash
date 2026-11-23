import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
    id: root

    signal sendMessage(string peerId, string message)
    signal startCall(string peerId)
    signal sendFile(string peerId, string filePath)
    signal openAttachment(string path)

    property string selectedPeerId: ""
    property string selectedPeerName: ""
    readonly property bool selfChat: selectedPeerId !== "" && selectedPeerId === backend.selfChatPeerId()
    /// True while a direct call with `selectedPeerId` is already up. Hides the
    /// dial button, exactly as Join Voice hides when voice is already in this
    /// room: the voice dock's hang-up is the end control, and a second dial would
    /// only stack another session on the live one.
    property bool callActiveWithPeer: false
    property var chatModel: null
    property var fileTransferModel: null
    property string typingPeerId: ""
    property bool peerIsTyping: false
    property bool youtubePreviewEnabled: true
    property var settingsModel: null
    property bool youtubeInlineAck: false
    property string _searchText: ""
    property bool _searchOpen: false
    property bool aiStreaming: false
    property string aiResponse: ""
    property string aiError: ""
    property int _aiSeq: 0
    property string _currentAiRequestId: ""
    property bool _statsPanelOpen: false
    property int _historyPage: 0
    property bool _loadingHistory: false
    property bool _hasMoreHistory: true
    property bool _pinnedToLatest: true

    onPeerIsTypingChanged: {
        if (root.peerIsTyping)
            typingClearTimer.restart()
        else
            typingClearTimer.stop()
    }

    Connections {
        target: backend
        function onConnectionStats(json) {
            try {
                var stats = JSON.parse(json)
                if (stats.peer_id !== root.selectedPeerId)
                    return
                connStatsPanel.applyStats(json)
                connStatsPanel.isRelay = backend.connection_mode === "relay"
            } catch (e) {}
        }
        function onOllamaChunk(requestId, text) {
            if (requestId === root._currentAiRequestId) root.aiResponse += text
        }
        function onOllamaDone(requestId) {
            if (requestId === root._currentAiRequestId) root.aiStreaming = false
        }
        function onOllamaError(requestId, error) {
            if (requestId === root._currentAiRequestId) {
                root.aiStreaming = false
                root.aiError = error
            }
        }
    }

    Timer {
        id: typingClearTimer
        interval: 4000
        onTriggered: {
            root.peerIsTyping = false
            root.typingPeerId = ""
        }
    }

    onSelectedPeerIdChanged: {
        root._historyPage = 0
        root._loadingHistory = false
        root._hasMoreHistory = true
        root._pinnedToLatest = true
        root._statsPanelOpen = false
        connStatsPanel.applyStats(JSON.stringify({
            peer_id: root.selectedPeerId,
            rtt_ms: 0,
            packet_loss_pct: 0,
            jitter_ms: 0,
            relay: backend.connection_mode === "relay",
            bandwidth_kbps: 0
        }))
    }

    StatsPanel {
        id: connStatsPanel
        z: 60
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.spacingMd
        anchors.topMargin: chatHeader.height + Theme.spacingMd
        visible: root._statsPanelOpen && root.selectedPeerId !== ""
    }

    MouseArea {
        z: 55
        anchors.fill: parent
        visible: root._statsPanelOpen
        onClicked: root._statsPanelOpen = false
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            id: chatHeader
            Layout.fillWidth: true
            // 48 at the designed size. The call button's tile grows with the
            // icon, and the name and status line with the text size; at large
            // text they outgrew a fixed height and spilled under the title bar.
            implicitHeight: Math.max(Theme.touchTarget + Theme.spacingXs, Theme.barActionSize,
                             chatHeaderRow.implicitHeight + Theme.spacingSm * 2)
            color: Theme.bg2

            RowLayout {
                id: chatHeaderRow
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: Theme.spacingMd
                anchors.rightMargin: Theme.spacingMd
                spacing: Theme.spacingSm

                Avatar {
                    visible: root.selectedPeerId !== ""
                    peerId: root.selectedPeerId
                    size: 28
                    showRing: true
                    Layout.alignment: Qt.AlignVCenter
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1

                    Text {
                        text: root.selectedPeerName || root.selectedPeerId || "No peer selected"
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeTitle
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    Text {
                        text: root.selectedPeerId === ""
                            ? "Choose a peer to begin"
                            : "Private trusted-peer conversation"
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeCaption
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }

                ConnectionStatsChip {
                    id: statsChip
                    Layout.alignment: Qt.AlignVCenter
                    peerId: root.selectedPeerId
                    rttMs: connStatsPanel.rttMs
                    packetLossPct: connStatsPanel.packetLossPct
                    isRelay: backend.connection_mode === "relay"
                    expanded: root._statsPanelOpen
                    onToggleExpanded: root._statsPanelOpen = !root._statsPanelOpen
                }

                IconButton {
                    icon.source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/search.svg")
                    icon.width: Theme.iconPx(18)
                    icon.height: Theme.iconPx(18)
                    visible: root.selectedPeerId !== ""
                    flat: true
                    ToolTip.text: "Search messages"
                    ToolTip.visible: hovered
                    onClicked: {
                        root._searchOpen = !root._searchOpen
                        if (root._searchOpen) msgSearchField.forceActiveFocus()
                        else {
                            root._searchText = ""
                            msgSearchField.clear()
                        }
                    }
                }

                // Start a call: solid green on a green square, the same
                // call control the Rooms tree uses.
                IconButton {
                    icon.source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/phone.svg")
                    icon.width: Theme.iconPx(18)
                    icon.height: Theme.iconPx(18)
                    icon.color: Theme.online
                    tileColor: Theme.semanticTint(Theme.online, 0.18)
                    tileBorder: Theme.online
                    visible: root.selectedPeerId !== "" && !root.callActiveWithPeer && !root.selfChat
                    Accessible.name: qsTr("Start call")
                    ToolTip.text: qsTr("Start call")
                    ToolTip.visible: hovered
                    onClicked: root.startCall(root.selectedPeerId)
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: root._searchOpen ? Theme.controlHeight + Theme.spacingXs : 0
            clip: true
            color: Theme.bg3
            Behavior on height { NumberAnimation { duration: Theme.animFast } }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Theme.spacingMd
                anchors.rightMargin: Theme.spacingSm
                spacing: Theme.spacingXs

                StyledTextField {
                    id: msgSearchField
                    Layout.fillWidth: true
                    placeholderText: "Search messages..."
                    onTextChanged: root._searchText = text.toLowerCase()
                    Keys.onEscapePressed: {
                        root._searchOpen = false
                        root._searchText = ""
                        clear()
                    }
                }

                Text {
                    visible: root._searchText !== "" && root.chatModel
                    text: {
                        if (!root.chatModel) return ""
                        var n = root.chatModel.matchCount(root._searchText)
                        return n === 1 ? "1 match" : (n + " matches")
                    }
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                }

                IconButton {
                    icon.source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/close.svg")
                    icon.width: Theme.iconPx(12)
                    icon.height: Theme.iconPx(12)
                    icon.color: Theme.muted
                    flat: true
                    onClicked: {
                        root._searchOpen = false
                        root._searchText = ""
                        msgSearchField.clear()
                    }
                }
            }
        }

        Shortcut {
            sequence: "Ctrl+F"
            enabled: root.selectedPeerId !== ""
            onActivated: {
                root._searchOpen = !root._searchOpen
                if (root._searchOpen) msgSearchField.forceActiveFocus()
                else {
                    root._searchText = ""
                    msgSearchField.clear()
                }
            }
        }

        ListView {
            id: msgList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.chatModel
            spacing: 2

            // Both distances are measured from `originY`, which prepending
            // older history moves - see JumpToCurrentButton.distanceFromLatest.
            // The button owns that arithmetic so the view that follows the
            // newest message and the button offering a way back to it can
            // never disagree about where it is.
            //
            // Only scrolling decides whether the reader has left the newest
            // message (`judgePin`). A geometry change - an image or link embed
            // loading to its real height, a row settling from its estimate,
            // the panel resizing - moves the end away from a stationary
            // `contentY`; judging pinned-ness from that unpinned the view a
            // few pixels short of the end, so it stopped following and Jump
            // to current took two presses. While pinned, those changes are
            // followed instead.
            function _restPosition(judgePin) {
                // `jumpToCurrent` is a child, so it is still undefined for
                // the layout passes that run while this view is being built.
                // An empty list reads as pinned there, which is where a
                // conversation should open anyway.
                if (judgePin)
                    root._pinnedToLatest = contentHeight <= height
                        || atYEnd
                        || (jumpToCurrent && jumpToCurrent.distanceFromLatest <= 24)
                if (root._loadingHistory || !root._hasMoreHistory || root.selectedPeerId === "")
                    return
                if (contentHeight > height && (atYBeginning || contentY - originY <= 48))
                    root.loadOlderHistory()
            }

            // True while `_followLatest` is moving the view, so the scroll it
            // causes is not mistaken for the reader's.
            property bool _following: false

            function _followLatest() {
                if (!root._pinnedToLatest)
                    return
                _following = true
                positionViewAtEnd()
                _following = false
            }

            function _geometryChanged() {
                if (root._pinnedToLatest)
                    Qt.callLater(msgList._followLatest)
                _restPosition(!root._pinnedToLatest)
            }

            onContentYChanged: _restPosition(!_following)
            onContentHeightChanged: _geometryChanged()
            onOriginYChanged: _geometryChanged()
            onHeightChanged: _geometryChanged()

            onCountChanged: {
                if (root._pinnedToLatest)
                    Qt.callLater(msgList._followLatest)
            }

            // A plain child of a ListView is parented to the view itself, not
            // to its contentItem, so this is already in viewport coordinates -
            // exactly like the empty state below. Anchoring parks it at the
            // bottom of the viewport; adding contentY would push it that far
            // past the bottom edge, where clip hides it at every scroll
            // position but the very top.
            HistoryAnchor {
                id: historyAnchor
                list: msgList
            }

            JumpToCurrentButton {
                id: jumpToCurrent
                list: msgList
                z: 2
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Theme.spacingMd
            }

            ColumnLayout {
                // Wider with larger text, so the line count stays about the
                // same; centred while it fits, from the top when it does not
                // (centring a taller block put its top under the header).
                anchors.horizontalCenter: parent.horizontalCenter
                y: Math.max(0, (parent.height - height) / 2)
                width: Math.min(parent.width - 48, Math.max(260, Theme.fontSizeBody * 20))
                spacing: 10
                visible: root.selectedPeerId === "" || msgList.count === 0

                Glyph {
                    source: root.selectedPeerId === ""
                        ? Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/speech.svg")
                        : Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/send.svg")
                    sourceSize.width: Theme.iconPx(40)
                    sourceSize.height: Theme.iconPx(40)
                    Layout.preferredWidth: Theme.iconPx(40)
                    Layout.preferredHeight: Theme.iconPx(40)
                    Layout.alignment: Qt.AlignHCenter
                    opacity: 0.72
                }

                Text {
                    text: root.selectedPeerId === "" ? "Select a peer" : "No messages yet"
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeTitle
                    font.bold: true
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }

                Text {
                    text: root.selectedPeerId === ""
                        ? "Choose a trusted peer from the left rail to open a private conversation."
                        : "Send a message or attach a file to start this conversation."
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    lineHeight: 1.3
                    Layout.fillWidth: true
                }
            }

            delegate: ChatRichMessageDelegate {
                property bool searchMatch: root._searchText === "" ||
                    (model.body || "").toLowerCase().indexOf(root._searchText) !== -1 ||
                    (model.attachmentName || "").toLowerCase().indexOf(root._searchText) !== -1
                property string dateSeparator: {
                    if (!searchMatch || (model.timestamp || 0) <= 0)
                        return ""
                    return root.dateSeparatorForIndex(index, model.timestamp)
                }
                visible: searchMatch
                height: searchMatch ? implicitHeight : 0
                clip: true
                msgId: model.msgId || ""
                sender: model.sender || ""
                body: model.body || ""
                kind: model.kind || "text"
                mine: !!model.mine
                timestamp: model.timestamp || 0
                status: model.status || "delivered"
                attachmentName: model.attachmentName || ""
                attachmentPath: model.attachmentPath || ""
                sizeStr: model.sizeStr || ""
                fileTransferModel: root.fileTransferModel
                inlinePreviewEnabled: root.youtubePreviewEnabled
                inlinePreviewAck: root.youtubeInlineAck
                allowDelete: true
                onInlineAckAccepted: {
                    root.youtubeInlineAck = true
                    if (root.settingsModel) {
                        root.settingsModel.youtube_inline_ack = true
                        root.settingsModel.save()
                    }
                }
                onCopyRequested: (text) => backend.copyToClipboard(text)
                onDeleteRequested: (id) => backend.deleteMessage(id)
                onRetryRequested: (id) => backend.retryMessage(id)
                onOpenAttachmentRequested: (path) => root.openAttachment(path)
                onTransferAcceptRequested: (id) => backend.acceptFile(id)
                onTransferRejectRequested: (id) => backend.rejectFile(id)
                onTransferRetryRequested: (id) => backend.retryFile(id)
            }
        }

        Item {
            Layout.fillWidth: true
            height: root.peerIsTyping && root.typingPeerId === root.selectedPeerId ? 22 : 0
            clip: true
            visible: height > 0
            Behavior on height { NumberAnimation { duration: 150 } }

            Text {
                anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
                text: (root.selectedPeerName || root.typingPeerId) + " is typing..."
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                font.italic: true
            }
        }

        Rectangle {
            id: aiPanel
            Layout.fillWidth: true
            Layout.margins: Theme.spacingSm
            implicitHeight: aiContent.implicitHeight + 16
            visible: root.aiResponse !== "" || root.aiStreaming || root.aiError !== ""
            color: Theme.bg1
            radius: 0
            border.color: Theme.accent
            border.width: 1

            ColumnLayout {
                id: aiContent
                anchors { left: parent.left; right: parent.right; margins: 10 }
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6

                RowLayout {
                    spacing: 6
                    Text {
                        text: "AI"
                        color: Theme.accent
                        font.pixelSize: Theme.fontSizeCaption
                        font.bold: true
                    }
                    Item { Layout.fillWidth: true }
                    StyledButton {
                        text: "Cancel"
                        visible: root.aiStreaming
                        compact: true
                        onClicked: {
                            backend.cancelOllama(root._currentAiRequestId)
                            root.aiStreaming = false
                        }
                    }
                    IconButton {
                        icon.source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/close.svg")
                        icon.width: Theme.iconPx(12)
                        icon.height: Theme.iconPx(12)
                        icon.color: Theme.muted
                        visible: !root.aiStreaming
                        flat: true
                        onClicked: {
                            root.aiResponse = ""
                            root.aiError = ""
                            root._currentAiRequestId = ""
                        }
                    }
                }

                Text {
                    visible: root.aiError !== ""
                    text: "Error: " + root.aiError
                    color: Theme.danger
                    font.pixelSize: Theme.fontSizeBody
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }

                Text {
                    visible: root.aiResponse !== ""
                    text: root.aiResponse
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }

                Text {
                    visible: root.aiStreaming && root.aiResponse === ""
                    text: "..."
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeBody
                    font.italic: true
                }
            }
        }

        RichChatComposer {
            Layout.margins: Theme.barMargin
            targetName: root.selectedPeerName || root.selectedPeerId
            targetKey: root.selectedPeerId
            enabledForTarget: root.selectedPeerId !== ""
            fileTransferEnabled: !root.selfChat
            aiEnabled: backend.ollama_available
            aiStreaming: root.aiStreaming
            onComposing: function(active) {
                if (root.selectedPeerId !== "") backend.sendTyping(root.selectedPeerId, active)
            }
            onSendMessage: function(message) {
                root.sendMessage(root.selectedPeerId, message)
                if (root.selectedPeerId !== "") backend.sendTyping(root.selectedPeerId, false)
            }
            onSendFile: function(fileUrl) {
                if (root.selectedPeerId !== "") root.sendFile(root.selectedPeerId, fileUrl)
            }
            onAskAi: function(prompt) {
                root._aiSeq += 1
                root._currentAiRequestId = "chat-" + root._aiSeq
                root.aiResponse = ""
                root.aiError = ""
                root.aiStreaming = true
                var sysPrompt = root.selectedPeerName !== ""
                    ? "You are helping " + root.selectedPeerName + " in a secure peer-to-peer chat."
                    : "You are an assistant in a secure peer-to-peer chat application."
                backend.askOllama(root._currentAiRequestId, prompt, sysPrompt)
                if (root.selectedPeerId !== "") backend.sendTyping(root.selectedPeerId, false)
            }
        }
    }

    function sameCalendarDay(a, b) {
        return a.getFullYear() === b.getFullYear()
            && a.getMonth() === b.getMonth()
            && a.getDate() === b.getDate()
    }

    function formatDateLabel(d) {
        var today = new Date()
        var yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1)
        if (root.sameCalendarDay(d, today))
            return "Today"
        if (root.sameCalendarDay(d, yesterday))
            return "Yesterday"
        return Theme.formatChatDate(d)
    }

    function dateSeparatorForIndex(idx, ts) {
        var d = new Date(ts * 1000)
        if (!root.chatModel)
            return root.formatDateLabel(d)
        var prevTs = root.chatModel.timestampAt(idx - 1)
        if (prevTs <= 0)
            return root.formatDateLabel(d)
        var prev = new Date(prevTs * 1000)
        if (root.sameCalendarDay(d, prev))
            return ""
        return root.formatDateLabel(d)
    }

    function loadOlderHistory() {
        if (root._loadingHistory || !root._hasMoreHistory || root.selectedPeerId === "")
            return
        root._loadingHistory = true
        root._historyPage += 1
        backend.loadMoreHistory(root.selectedPeerId, root._historyPage)
    }

    function onHistoryPrepended(json) {
        var rows = []
        try { rows = JSON.parse(json) } catch (e) { rows = [] }
        if (!rows.length) {
            root._hasMoreHistory = false
            root._loadingHistory = false
            return
        }
        // Captured before the insert renumbers everything, and restored
        // while `_loadingHistory` is still set so that landing near the top
        // again cannot immediately ask for another page.
        var anchor = historyAnchor.capture()
        if (root.chatModel) {
            root.chatModel.prependMessages(json)
            historyAnchor.restore(anchor, rows.length)
        }
        root._hasMoreHistory = rows.length >= 50
        root._loadingHistory = false
    }
}
