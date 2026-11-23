import QtQuick
import QtQuick.Controls
import QtQuick.Window
import DoubleSlash.Client 1.0

/*!
    Drives the running client through its screens and checks every item drawn
    there against layout, legibility and design rules.

    Loaded by MainWindow only when the process is started with
    \c {--ui-audit=<dir>}, normally through \c scripts/test_ui_audit.ps1, which
    also gives it a throwaway profile. Nothing here runs in an ordinary launch.

    Each stop on the tour is a \e scene: the Peers and Rooms lists, a sample
    one-to-one and room conversation, every Settings section and the dialogs
    and wizards that can be opened without a peer. A scene whose main list or
    page scrolls is audited page by page from top to bottom, so items below
    the fold are checked too. The tour repeats for each \e variant: base
    palette, skin, text size and window size, the cases DESIGN.md asks every UI
    change to be checked under.

    Results go to \c report.json, \c report.html (screenshots with each finding
    outlined) and \c shots/. The audit is observational: it never sends an
    invite, joins a room or saves anything except the onboarding flag of the
    throwaway profile, and it deliberately skips the onboarding step that would
    register the URI scheme and create desktop shortcuts.

    Writing the report needs \c QML_XHR_ALLOW_FILE_WRITE=1 (and an allowlist
    needs \c QML_XHR_ALLOW_FILE_READ=1); the runner script sets both.
*/
Item {
    id: audit
    visible: false
    width: 0
    height: 0

    /*! The MainWindow being audited. */
    required property var app
    /*! Objects from MainWindow this tour drives, by their MainWindow id. */
    required property var refs
    /*! Where the report and screenshots are written. Created by the runner. */
    required property string outDir
    /*! \c quick (4 variants) or \c full (every skin and text size). */
    property string matrix: "quick"
    /*! Only scenes whose name matches this regular expression, when set. */
    property string onlyPattern: ""
    /*! Optional JSON allowlist of accepted findings. */
    property string allowPath: ""
    /*!
        Optional "x,y" in window coordinates: every page then records what is
        painted under that point, top first, as \c probe in the report. For
        working out where a colour on a screenshot comes from.
    */
    property string probe: ""

    readonly property string passphrase: "ui-audit-throwaway-profile"

    property var report: null
    property var _allow: []
    property var _variant: null
    property var _scene: null
    property int _shotSeq: 0
    property string _sampleImage: ""
    property var _steps: []
    property int _stepIndex: 0
    property bool _finished: false

    Timer {
        id: stepTimer
        repeat: false
        onTriggered: audit._runStep()
    }

    // An async step whose callback never comes (a grab of a window that is
    // not exposed, say) must not hang the run.
    Timer {
        id: asyncWatchdog
        property int stepIndex: -1
        interval: 8000
        repeat: false
        onTriggered: {
            if (audit._stepIndex !== stepIndex)
                return
            audit._error("async step timed out: " + (audit._steps[stepIndex].label || ""))
            audit._stepIndex++
            audit._schedule(0)
        }
    }

    Component.onCompleted: Qt.callLater(audit.start)

    // ── Step runner ─────────────────────────────────────────────────────────
    //
    // A step is { wait: ms }, { call: fn } (fn may return more steps to run
    // next), { until: fn, timeout, label, fatal } or { async: fn(done) }.

    function _schedule(ms) {
        stepTimer.interval = Math.max(0, ms)
        stepTimer.restart()
    }

    function _insert(more) {
        if (!more || !more.length)
            return
        Array.prototype.splice.apply(_steps, [_stepIndex, 0].concat(more))
    }

    function _runStep() {
        if (_finished)
            return
        if (_stepIndex >= _steps.length) {
            _finish()
            return
        }
        var s = _steps[_stepIndex]
        try {
            if (s.wait !== undefined) {
                _stepIndex++
                _schedule(s.wait)
            } else if (s.until) {
                if (s.started === undefined)
                    s.started = Date.now()
                var ok = false
                try { ok = !!s.until() } catch (e) { ok = false }
                if (ok) {
                    _stepIndex++
                    _schedule(0)
                } else if (Date.now() - s.started > (s.timeout || 10000)) {
                    _error("timed out waiting for " + (s.label || "a condition"))
                    _stepIndex++
                    if (s.fatal)
                        _finish()
                    else
                        _schedule(0)
                } else {
                    _schedule(100)
                }
            } else if (s.call) {
                var more = s.call()
                _stepIndex++
                _insert(more)
                _schedule(0)
            } else if (s.async) {
                var idx = _stepIndex
                asyncWatchdog.stepIndex = idx
                asyncWatchdog.restart()
                s.async(function(more) {
                    if (audit._stepIndex !== idx || audit._finished)
                        return
                    asyncWatchdog.stop()
                    audit._stepIndex++
                    audit._insert(more)
                    audit._schedule(0)
                })
            } else {
                _stepIndex++
                _schedule(0)
            }
        } catch (e) {
            _error("step " + (s.label || _stepIndex) + " threw: " + e + (e.stack ? "\n" + e.stack : ""))
            _stepIndex++
            _schedule(0)
        }
    }

    function _error(msg) {
        console.warn("[ui-audit] " + msg)
        if (report)
            report.errors.push({ variant: _variant ? _variant.id : "", scene: _scene ? _scene.name : "", message: String(msg) })
    }

    // ── Plan ────────────────────────────────────────────────────────────────

    function _variants() {
        var quick = [
            { id: "dark", label: "Dark, designed size", theme: "dark", skin: "default", font: 0, w: 1100, h: 700 },
            { id: "light", label: "Light, designed size", theme: "light", skin: "default", font: 0, w: 1100, h: 700 },
            { id: "dark-text200", label: "Dark, text +100%", theme: "dark", skin: "default", font: 100, w: 1100, h: 700 },
            { id: "dark-narrow", label: "Dark, minimum window", theme: "dark", skin: "default", font: 0, w: app.minimumWidth, h: app.minimumHeight }
        ]
        // Dark-based variants run first. After a palette change grabToImage
        // can paint some transparent rectangles white - the window buttons'
        // always, a whole sidebar after light to dark - while the window
        // itself is drawn correctly (checked against a screen capture). The
        // audit reads the scene, not pixels, so only screenshots suffer, and
        // this order keeps the worst case out of them.
        var darkFirst = function(list) {
            return list.filter(function(v) { return v.theme !== "light" })
                       .concat(list.filter(function(v) { return v.theme === "light" }))
        }
        if (matrix !== "full")
            return darkFirst(quick)
        var full = quick.concat([
            { id: "dark-text50", label: "Dark, text -50%", theme: "dark", skin: "default", font: -50, w: 1100, h: 700 },
            { id: "dark-text300", label: "Dark, text +200%", theme: "dark", skin: "default", font: 200, w: 1100, h: 700 },
            { id: "light-text200-narrow", label: "Light, text +100%, minimum window", theme: "light", skin: "default", font: 100, w: app.minimumWidth, h: app.minimumHeight }
        ])
        for (var i = 0; i < Theme.presets.length; i++) {
            var p = Theme.presets[i]
            if (p.id === "default")
                continue
            full.push({ id: "skin-" + p.id, label: "Skin " + p.name, theme: p.base === "light" ? "light" : "dark",
                        skin: p.id, font: 0, w: 1100, h: 700 })
        }
        return darkFirst(full)
    }

    function _wanted(name) {
        if (onlyPattern === "")
            return true
        try { return new RegExp(onlyPattern).test(name) } catch (e) { return true }
    }

    function start() {
        report = {
            started: new Date().toISOString(),
            version: Qt.application.version,
            platform: Qt.platform.os,
            matrix: matrix,
            only: onlyPattern,
            screen: { w: Screen.width, h: Screen.height, dpr: Screen.devicePixelRatio },
            qpa: Qt.platform.pluginName,
            variants: [],
            errors: []
        }
        // Without a real window system there are no system fonts (every
        // glyph is a box) and the screen is invented, so text sizes and
        // positions are meaningless. Fail instead of reporting them.
        if (Qt.platform.pluginName === "offscreen" || Qt.platform.pluginName === "minimal") {
            _error("running on the '" + Qt.platform.pluginName + "' platform, where text cannot be measured; "
                + "unset QT_QPA_PLATFORM and run on a desktop session")
            _finish()
            return
        }
        _loadAllow()
        var steps = [
            { wait: 1500 },
            { until: function() { return refs.passphraseDialog.visible || refs.backend.public_id !== "" },
              timeout: 30000, label: "the passphrase screen or an unlocked identity", fatal: true },
            { call: audit._startupSteps }
        ]
        var variants = _variants()
        for (var i = 0; i < variants.length; i++)
            steps.push({ call: (function(v) { return function() { return audit._variantSteps(v) } })(variants[i]) })
        _steps = steps
        _stepIndex = 0
        _schedule(0)
    }

    /// The first run of a profile: the passphrase screen, then onboarding.
    function _startupSteps() {
        _beginVariant({ id: "startup", label: "First launch (dark, designed size)", theme: "dark", skin: "default",
                        font: 0, w: app.width, h: app.height })
        var steps = []
        if (refs.backend.public_id === "") {
            if (_wanted("passphrase"))
                steps = steps.concat(_captureSteps("passphrase", function() { return refs.passphraseDialog }))
            steps.push({ call: function() {
                // `remember` false: never write this throwaway identity's key
                // to the OS keyring.
                refs.passphraseDialog.visible = false
                refs.backend.unlockWithPassphraseAndFile(audit.passphrase, "", false)
            } })
            steps.push({ until: function() { return refs.backend.public_id !== "" },
                         timeout: 90000, label: "the identity to unlock", fatal: true })
            steps.push({ wait: 1200 })
        }
        steps.push({ call: function() {
            // Onboarding opens itself on a fresh profile; it is audited with
            // every variant below, so it only needs to be put away here -
            // without finishWizard(), which would register the URI scheme and
            // create desktop shortcuts for this build.
            refs.settingsModel.onboarding_complete = true
            refs.settingsModel.save()
            if (refs.onboardingWizard.opened)
                refs.onboardingWizard.close()
        } })
        steps.push({ wait: 600 })
        return steps
    }

    function _beginVariant(v) {
        _variant = { id: v.id, label: v.label, theme: v.theme, skin: v.skin, font: v.font,
                     size: { w: v.w, h: v.h }, scenes: [] }
        report.variants.push(_variant)
    }

    function _applyVariant(v) {
        if (app.visibility === Window.Maximized || app.visibility === Window.FullScreen)
            app.showNormal()
        app.width = v.w
        app.height = v.h
        var preset = null
        for (var i = 0; i < Theme.presets.length; i++)
            if (Theme.presets[i].id === v.skin)
                preset = Theme.presets[i]
        app.applyThemePreference(v.theme)
        Theme.applySkinJson(preset && preset.id !== "default"
            ? Theme.skinJson(preset.name, preset.base, preset.colors) : "")
        app.applyFontScale(v.font)
    }

    function _variantSteps(v) {
        var steps = [
            { call: function() { audit._beginVariant(v); audit._applyVariant(v) } },
            { wait: 900 }
        ]
        var scenes = _sceneList()
        for (var i = 0; i < scenes.length; i++) {
            if (_wanted(scenes[i].name))
                steps = steps.concat(_sceneSteps(scenes[i]))
        }
        return steps
    }

    // ── Scenes ──────────────────────────────────────────────────────────────

    function _popupScope(p) {
        if (!p)
            return null
        // A Popup's contentItem and background live in its popup item, which
        // Qt parents to the window overlay; that item is what is on screen.
        if (p.contentItem !== undefined && p.opened !== undefined && p.contentItem)
            return p.contentItem.parent
        return p
    }

    function _closePopup(p) {
        if (!p)
            return
        if (typeof p.close === "function")
            p.close()
        else
            p.visible = false
    }

    function _page() { return app.contentItem }

    function _sceneList() {
        var r = refs
        var list = []

        list.push({ name: "peers", scope: _page, setup: function() {
            app.navIndex = 0
            app.sidebarTab = 0
            r.chatPanel.selectedPeerId = ""
            r.chatPanel.selectedPeerName = ""
            // The previous variant's sample conversation would otherwise
            // still be in the list under "Select a peer".
            r.chatModel.clearMessages()
        } })

        list.push({ name: "self-chat", scope: _page, settle: 900, setup: function() {
            app.sidebarTab = 0
            r.chatPanel.selectedPeerId = r.backend.selfChatPeerId()
            r.chatPanel.selectedPeerName = qsTr("Message myself")
            app.navIndex = 0
        }, populate: function() {
            // Display only: setMessages fills the view's model and stores nothing.
            r.chatModel.setMessages(JSON.stringify(audit._sampleMessages(false)))
        } })

        list.push({ name: "clear-peer-history", scope: function() {
            if (r.chatModel.matchCount("clear-history-sentinel") !== 0)
                audit._error("Cleared peer messages stayed visible until another peer was selected")
            if (r.chatPanel.selectedPeerId !== "ui-audit-clear-open")
                audit._error("Clearing history changed the selected peer")
            if (r.chatPanel._historyPage !== 0 || r.chatPanel._loadingHistory)
                audit._error("Clearing history did not reset the active history page")
            return _page()
        }, setup: function() {
            app.sidebarTab = 0
            app.navIndex = 0
            r.chatPanel.selectedPeerId = "ui-audit-clear-open"
            r.chatPanel.selectedPeerName = "History clear test"
            r.backend.selectPeer(r.chatPanel.selectedPeerId)
        }, populate: function() {
            r.chatModel.setMessages(JSON.stringify([
                { msg_id: "clear-in", body: "clear-history-sentinel received", mine: false, timestamp: 1, kind: "text", status: "delivered" },
                { msg_id: "clear-out", body: "clear-history-sentinel sent", mine: true, timestamp: 2, kind: "text", status: "delivered" }
            ]))
            r.chatPanel._historyPage = 4
            r.chatPanel._loadingHistory = true
            r.backend.clearPeerHistory("ui-audit-clear-other")
            if (r.chatModel.matchCount("clear-history-sentinel") !== 2)
                audit._error("Clearing an unselected peer erased the open conversation")
            r.backend.clearPeerHistory(r.chatPanel.selectedPeerId)
        } })

        list.push({ name: "rooms", scope: _page, setup: function() {
            audit._injectSampleRooms()
            app.sidebarTab = 1
            app.navIndex = 1
        } })

        list.push({ name: "block-purge-confirmation", dialog: true,
            scope: function() { return audit._popupScope(r.confirmBlockPurgeDialog) },
            setup: function() {
                app.navIndex = 0
                r.confirmBlockPurgeDialog.ask("A".repeat(43), audit._longName)
            }, teardown: function() {
                audit._namedItem(r.confirmBlockPurgeDialog.contentItem, "cancelBlockPurgeButton").clicked()
                if (JSON.parse(r.backend.blocked_people_json).some(function(person) { return person.public_id === "A".repeat(43) }))
                    audit._error("Opening and cancelling the purge confirmation blocked the person")
            }
        })

        list.push({ name: "block-purge-history", ready: function() {
            return r.backend.session_banner.indexOf("Blocked; purged ") === 0
        }, scope: function() {
            if (r.chatModel.matchCount("purge-private-sentinel") !== 0)
                audit._error("Purging did not immediately clear the open private conversation")
            if (r.chatPanel.selectedPeerId !== "A".repeat(43))
                audit._error("Purging changed the selected private conversation")
            var list = audit._namedItem(r.roomPanel, "roomChatMessages")
            if (!list || list.model.count !== 2)
                audit._error("Purging did not preserve exactly the other authors' room messages")
            else for (var i = 0; i < list.model.count; i++) {
                if (list.model.get(i).senderPeerId.replace(/=+$/, "") === "A".repeat(43))
                    audit._error("Purged room author stayed visible")
            }
            return _page()
        }, setup: function() {
            app.sidebarTab = 1
            app.navIndex = 1
            r.chatPanel.selectedPeerId = "A".repeat(43)
            r.chatPanel.selectedPeerName = "Purge test"
            r.backend.selectPeer(r.chatPanel.selectedPeerId)
            r.roomPanel.switchToRoom("Purge test room", "ui-audit-purge-room", "ui-audit-node-1")
        }, populate: function() {
            r.chatModel.setMessages(JSON.stringify([
                {msg_id: "purge-in", body: "purge-private-sentinel received", mine: false, timestamp: 1, kind: "text", status: "delivered"},
                {msg_id: "purge-out", body: "purge-private-sentinel reply", mine: true, timestamp: 2, kind: "text", status: "delivered"}
            ]))
            var authors = ["A".repeat(43) + "=", "other-room-author", r.backend.public_id]
            for (var i = 0; i < authors.length; i++) {
                r.roomPanel.appendRoomChat(JSON.stringify({msg_id: "purge-room-" + i, sender_id: authors[i],
                    sender: i === 0 ? "Blocked author" : (i === 1 ? "Other member" : "Me"),
                    body: "Room message from " + (i === 0 ? "blocked author" : (i === 1 ? "another member" : "me")),
                    mine: i === 2, timestamp: 1000 + i, kind: "text"}))
            }
            r.confirmBlockPurgeDialog.ask("A".repeat(43), "Purge test")
            audit._namedItem(r.confirmBlockPurgeDialog.contentItem, "confirmBlockPurgeButton").clicked()
        }, teardown: function() {
            r.backend.unblockPeer("A".repeat(43))
        } })

        list.push({ name: "room-chat", scope: _page, settle: 900, setup: function() {
            audit._injectSampleRooms()
            app.sidebarTab = 1
            app.navIndex = 1
            r.roomPanel.switchToRoom(audit._longRoomName, "ui-audit-room-1", "ui-audit-node-1")
        }, populate: function() {
            var rows = audit._sampleMessages(true)
            for (var i = 0; i < rows.length; i++) {
                var m = rows[i]
                m.room_id = "ui-audit-room-1"
                m.supernode_id = "ui-audit-node-1"
                r.roomPanel.appendRoomChat(JSON.stringify(m))
            }
        } })

        var tabs = [
            [r.settingsPage.tabAudio, "audio"], [r.settingsPage.tabVideo, "video"],
            [r.settingsPage.tabIdentity, "identity"], [r.settingsPage.tabGeneral, "general"],
            [r.settingsPage.tabAi, "ai"], [r.settingsPage.tabNetwork, "network"],
            [r.settingsPage.tabSecurity, "security"], [r.settingsPage.tabPrivacy, "privacy"],
            [r.settingsPage.tabDiagnostics, "diagnostics"], [r.settingsPage.tabAbout, "about"]
        ]
        for (var t = 0; t < tabs.length; t++) {
            list.push((function(tab, name) {
                var blockedBefore = "[]"
                return { name: "settings-" + name, scope: _page, setup: function() {
                    if (name === "privacy") {
                        blockedBefore = r.backend.blocked_people_json
                        r.backend.blocked_people_json = JSON.stringify([
                            { public_id: "A".repeat(43), label: "A room member" },
                            { public_id: "B".repeat(43), label: audit._longName }
                        ])
                    }
                    app.navIndex = 2
                    app.settingsTab = tab
                }, teardown: function() {
                    if (name === "privacy") r.backend.blocked_people_json = blockedBefore
                    // Leaving Settings stops any audio test a section started.
                    app.leaveSettings()
                } }
            })(tabs[t][0], tabs[t][1]))
        }

        var wizard = r.onboardingWizard
        var steps = wizard.stepCount || 2
        for (var s = 0; s < steps; s++) {
            list.push((function(step) {
                return { name: "onboarding-step" + (step + 1), dialog: true, settle: 700,
                    scope: function() { return audit._popupScope(wizard) },
                    setup: function() { wizard.step = step; if (!wizard.opened) wizard.open() },
                    teardown: function() { if (step === steps - 1) { wizard.close(); wizard.step = 0 } } }
            })(s))
        }

        var dialogs = [
            ["create-room", r.createRoomDialog],
            ["accept-invite", r.acceptInviteDialog],
            ["invite-popup", r.invitePopup],
            ["backup-wizard", r.backupWizard],
            ["host-supernode-wizard", r.hostSupernodeWizard]
        ]
        for (var d = 0; d < dialogs.length; d++) {
            list.push((function(name, dlg) {
                return { name: name, dialog: true, settle: 700,
                    scope: function() { return audit._popupScope(dlg) },
                    setup: function() {
                        app.navIndex = 0
                        if (typeof dlg.open === "function") dlg.open(); else dlg.visible = true
                    },
                    teardown: function() { audit._closePopup(dlg) } }
            })(dialogs[d][0], dialogs[d][1]))
        }
        return list
    }

    function _sceneSteps(sc) {
        var steps = [
            { label: sc.name + " setup", call: function() { audit._scene = { name: sc.name, pages: [] }; audit._variant.scenes.push(audit._scene); sc.setup() } },
            { wait: sc.settle || 600 }
        ]
        if (sc.populate) {
            steps.push({ label: sc.name + " populate", call: sc.populate })
            steps.push({ wait: 900 })
        }
        if (sc.ready)
            steps.push({label: sc.name + " completion", until: sc.ready, timeout: 10000})
        steps.push({ label: sc.name + " pages", call: function() { return audit._pageSteps(sc, 0, null) } })
        if (sc.teardown)
            steps.push({ label: sc.name + " teardown", call: sc.teardown }, { wait: 350 })
        return steps
    }

    function _namedItem(root, name) {
        if (root.objectName === name) return root
        var children = root.children || []
        for (var i = 0; i < children.length; i++) {
            var item = _namedItem(children[i], name)
            if (item) return item
        }
        return null
    }

    /// Audits the current page, then scrolls the scene's main list one
    /// viewport further and does the same until its end.
    function _pageSteps(sc, page, flick) {
        var scope = sc.scope()
        if (!scope || !scope.visible) {
            _error("scene " + sc.name + " has nothing on screen to audit")
            return []
        }
        if (page === 0) {
            flick = _mainScroller(scope)
            if (flick)
                flick.contentY = flick.originY
        }
        var steps = []
        if (page === 0 && flick)
            steps.push({ wait: 300 })
        steps = steps.concat(_captureSteps(sc.name, sc.scope, page))
        if (flick && page < 30) {
            var end = flick.originY + flick.contentHeight - flick.height
            if (flick.contentY < end - 2) {
                steps.push({ call: function() {
                    var stepPx = Math.max(40, flick.height * 0.85)
                    flick.contentY = Math.min(end, flick.contentY + stepPx)
                } }, { wait: 300 })
                steps.push({ call: function() { return audit._pageSteps(sc, page + 1, flick) } })
            }
        }
        return steps
    }

    /// The largest vertically scrolling list or page in `scope`, if any.
    function _mainScroller(scope) {
        var best = null
        var bestArea = 0
        function visit(it) {
            if (!it || !it.visible)
                return
            if (audit._isFlickable(it) && it.contentHeight > it.height + 4 && it.height > 120
                    && it.orientation !== ListView.Horizontal) {
                var a = it.width * it.height
                if (a > bestArea) { bestArea = a; best = it }
            }
            var kids = it.children
            for (var i = 0; i < kids.length; i++)
                visit(kids[i])
        }
        visit(scope)
        return best
    }

    function _captureSteps(sceneName, scopeFn, page) {
        var shot = ""
        return [
            { label: sceneName + " audit", call: function() {
                if (!audit._scene || audit._scene.name !== sceneName) {
                    audit._scene = { name: sceneName, pages: [] }
                    audit._variant.scenes.push(audit._scene)
                }
                shot = audit._shotName(sceneName, page || 0)
                audit._auditPage(scopeFn(), shot, page || 0)
            } },
            { label: sceneName + " screenshot", async: function(done) {
                audit._grab(shot, done)
            } }
        ]
    }

    function _shotName(scene, page) {
        _shotSeq++
        var base = (_variant ? _variant.id : "x") + "__" + scene + (page > 0 ? "__p" + (page + 1) : "")
        return "shots/" + base.replace(/[^A-Za-z0-9_.-]/g, "_") + ".png"
    }

    function _rootItem() {
        var it = app.contentItem
        while (it && it.parent)
            it = it.parent
        return it
    }

    // grabToImage renders the item tree, not the window: wherever nothing is
    // painted the grab is transparent, which an image viewer shows as white
    // (with colour fringes on sub-pixel text). The window colour goes under
    // the grab so the screenshot matches what is on screen.
    Component {
        id: backdropComponent
        Rectangle { z: -1000000 }
    }

    function _grab(shot, done) {
        var root = _rootItem()
        var backdrop = backdropComponent.createObject(root, { width: root.width, height: root.height, color: app.color })
        var finish = function() {
            if (backdrop)
                backdrop.destroy()
            done()
        }
        var ok = root.grabToImage(function(result) {
            var path = audit.outDir + "/" + shot
            if (!result.saveToFile(path))
                audit._error("could not save " + path)
            else if (audit._sampleImage === "")
                audit._sampleImage = path
            finish()
        })
        if (!ok) {
            _error("grabToImage refused for " + shot)
            finish()
        }
    }

    // ── Sample content ─────────────────────────────────────────────────────

    readonly property string _longRoomName: "Weekly planning, retro and the very long room name that never ends"
    readonly property string _longName: "Maximiliana Wolfeschlegelsteinhausenbergerdorff"

    function _injectSampleRooms() {
        var model = refs.nodeListModel
        for (var i = 0; i < model.count; i++)
            if (model.get(i).node_id === "ui-audit-node-1")
                return
        var member = function(id, name) {
            return { id: id, name: name, trusted: false, list_peer_id: "", is_self: false }
        }
        var rooms = [
            { room_id: "default", name: "default", kind: "voice", is_default: true, voice_count: 0, count_known: true,
              chat_count: 0, chat_count_known: true, known_peers: [], unknown_peers: 0, parent_id: "" },
            { room_id: "ui-audit-room-1", name: audit._longRoomName, kind: "voice", voice_count: 3, count_known: true,
              chat_count: 2, chat_count_known: true, known_peers: [], unknown_peers: 3, parent_id: "",
              voice_members: [member("a1", audit._longName), member("a2", "Bo"), member("a3", "Chandrasekhar Venkataraman")],
              text_members: [member("a4", "Dee"), member("a5", "Élodie Fontaine-Beaumarchais")] },
            { room_id: "ui-audit-room-2", name: "Sub-room with a fairly long name too", kind: "voice", voice_count: 0,
              count_known: true, chat_count: 0, chat_count_known: true, known_peers: [], unknown_peers: 0,
              parent_id: "ui-audit-room-1" },
            { room_id: "ui-audit-room-3", name: "x", kind: "voice", voice_count: 12, count_known: true,
              chat_count: 140, chat_count_known: true, known_peers: [], unknown_peers: 12, parent_id: "" }
        ]
        model.append({
            node_id: "ui-audit-node-1",
            connected: true,
            homepage_url: "",
            title: "Sample supernode with an operator title that runs long",
            local_name: "",
            sfu_enabled: true,
            public_rooms_enabled: true,
            rooms_json: JSON.stringify(rooms)
        })
        if (typeof app.recountRooms === "function")
            app.recountRooms()
    }

    function _sampleMessages(room) {
        var now = Date.now() / 1000
        var other = room ? audit._longName : "Message myself"
        var rows = [
            { sender: other, body: "Short one.", mine: false },
            { sender: "Me", body: "A normal reply that wraps onto a second line once the window is narrow enough to need it.", mine: true },
            { sender: other, body: "Unbroken: " + "a1b2c3d4e5f6".repeat(14), mine: false },
            { sender: other, body: "https://example.com/a/rather/long/path/that/goes/on/and/on/without/any/break/at/all?with=query&and=more", mine: false },
            { sender: "Me", body: "Line one\nLine two\nLine three\n\nAfter a blank line", mine: true, status: "failed" },
            { sender: "Me", body: "Sending…", mine: true, status: "sending" },
            { sender: other, body: "Emoji 😀🎉👍🏽 and ümlauts, 中文, العربية", mine: false },
            { sender: other, body: "", mine: false, kind: "file", attachment_name: "quarterly-report-final-FINAL-v7-with-a-very-long-file-name.pdf",
              attachment_path: "", size_str: "12.4 MB" },
            { sender: "Me", body: "", mine: true, kind: "image", attachment_name: "screenshot.png",
              attachment_path: audit._sampleImage, size_str: "240 KB" },
            { sender: other, body: "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(8), mine: false }
        ]
        for (var i = 0; i < rows.length; i++) {
            var m = rows[i]
            m.msg_id = "ui-audit-" + (room ? "r" : "d") + i
            m.timestamp = now - (rows.length - i) * 3700
            m.kind = m.kind || "text"
            m.status = m.status || "delivered"
            m.sender_id = m.mine ? "" : "ui-audit-peer"
        }
        return rows
    }

    // ── The audit itself ───────────────────────────────────────────────────

    function _isText(it) { return typeof it.elide === "number" && typeof it.truncated === "boolean" && typeof it.text === "string" }
    function _isTextInput(it) { return typeof it.echoMode === "number" && typeof it.cursorPosition === "number" }
    function _isRect(it) { return typeof it.radius === "number" && it.border !== undefined && it.border !== null && typeof it.border.width === "number" }
    function _isFlickable(it) { return typeof it.contentY === "number" && typeof it.flickableDirection === "number" }
    function _isButton(it) { return typeof it.autoRepeat === "boolean" && typeof it.checkable === "boolean" && typeof it.pressed === "boolean" }
    function _isMouseArea(it) { return typeof it.acceptedButtons === "number" && typeof it.containsMouse === "boolean" && typeof it.pressAndHoldInterval === "number" }
    function _isImage(it) { return typeof it.fillMode === "number" && it.source !== undefined && typeof it.status === "number" }

    /// "SettingsCard" from "SettingsCard_QMLTYPE_12(0x…)", "Rectangle" from
    /// "QQuickRectangle(0x…)". `custom` is true for types written in QML.
    function _typeOf(it) {
        var s = String(it)
        var m = /^([A-Za-z0-9_]+?)(_QMLTYPE_\d+|_QML_\d+)?\(/.exec(s)
        var name = m ? m[1] : s
        var custom = !!(m && m[2])
        if (name.indexOf("QQuick") === 0)
            name = name.substring(6)
        return { name: name, custom: custom }
    }

    readonly property var _roundControls: ["ToolButton", "RoundButton", "Switch", "SwitchDelegate", "Slider", "RangeSlider",
        "BusyIndicator", "RadioButton", "RadioDelegate", "Dial"]

    function _rect(x, y, w, h) { return { x: x, y: y, w: w, h: h } }
    function _inter(a, b) {
        var x = Math.max(a.x, b.x), y = Math.max(a.y, b.y)
        var r = Math.min(a.x + a.w, b.x + b.w), btm = Math.min(a.y + a.h, b.y + b.h)
        return _rect(x, y, Math.max(0, r - x), Math.max(0, btm - y))
    }
    function _contains(outer, inner, tol) {
        return inner.x >= outer.x - tol && inner.y >= outer.y - tol
            && inner.x + inner.w <= outer.x + outer.w + tol && inner.y + inner.h <= outer.y + outer.h + tol
    }
    function _area(r) { return r.w * r.h }
    function _round(r) { return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) } }

    /// Walks the whole window in paint order. Items outside `scope` are kept
    /// only as backgrounds for the contrast check.
    function _walk(scope) {
        var nodes = []
        var root = _rootItem()
        var inf = 1e7
        var windowRect = _rect(0, 0, root.width, root.height)
        function visit(it, ctx) {
            if (!it || !it.visible)
                return
            var op = ctx.opacity * it.opacity
            if (op < 0.02)
                return
            var m = it.mapToItem(null, 0, 0, it.width, it.height)
            var rect = _rect(m.x, m.y, m.width, m.height)
            var inScope = ctx.inScope || it === scope
            var node = {
                item: it, rect: rect, opacity: op, inScope: inScope, parent: ctx.node,
                clipAll: ctx.clipAll, clipStatic: ctx.clipStatic,
                vis: _inter(rect, ctx.clipAll), scroller: ctx.scroller
            }
            // Content that scrolls belongs to its Flickable's contentItem;
            // anything else declared on a ListView (a jump button, an empty
            // state) floats over it on purpose.
            if (ctx.node && _isFlickable(ctx.node.item) && it === ctx.node.item.contentItem)
                node.scroller = node
            var kids = []
            var ch = it.children
            for (var i = 0; i < ch.length; i++)
                kids.push({ item: ch[i], z: ch[i].z, i: i })
            kids.sort(function(a, b) { return a.z !== b.z ? a.z - b.z : a.i - b.i })

            var childAll = ctx.clipAll
            var childStatic = ctx.clipStatic
            if (_isFlickable(it)) {
                // Content in a scrolling axis is meant to run past every
                // ancestor; only the axes that do not scroll still constrain it.
                var vScroll = it.contentHeight > it.height + 1
                var hScroll = it.contentWidth > it.width + 1
                childStatic = _rect(hScroll ? -inf : childStatic.x, vScroll ? -inf : childStatic.y,
                                    hScroll ? 2 * inf : childStatic.w, vScroll ? 2 * inf : childStatic.h)
                var bound = _rect(hScroll ? -inf : rect.x, vScroll ? -inf : rect.y,
                                  hScroll ? 2 * inf : rect.w, vScroll ? 2 * inf : rect.h)
                childStatic = _inter(childStatic, bound)
                if (it.clip)
                    childAll = _inter(childAll, rect)
            } else if (it.clip) {
                childAll = _inter(childAll, rect)
                childStatic = _inter(childStatic, rect)
            }
            var childCtx = { opacity: op, inScope: inScope, node: node, clipAll: childAll, clipStatic: childStatic,
                             scroller: node.scroller }
            var k = 0
            for (; k < kids.length && kids[k].z < 0; k++)
                visit(kids[k].item, childCtx)
            node.paint = nodes.length
            nodes.push(node)
            for (; k < kids.length; k++)
                visit(kids[k].item, childCtx)
        }
        visit(root, { opacity: 1, inScope: false, node: null, clipAll: windowRect, clipStatic: windowRect, scroller: null })
        return nodes
    }

    function _path(node) {
        var parts = []
        for (var n = node; n; n = n.parent) {
            var it = n.item
            var t = _typeOf(it)
            if (n === node || it.objectName || t.custom)
                parts.unshift(it.objectName ? t.name + "#" + it.objectName : t.name)
            if (n.item === app)
                break
        }
        if (parts.length > 7)
            parts = parts.slice(parts.length - 7)
        return parts.join(" › ")
    }

    function _plain(s) {
        return String(s || "").replace(/<[^>]*>/g, "").replace(/\s+/g, " ").trim()
    }

    function _label(it) {
        if (_isText(it))
            return _plain(it.text).substring(0, 100)
        if (_isButton(it)) {
            var t = _plain(it.text)
            if (t !== "")
                return t.substring(0, 100)
            var src = it.icon && it.icon.source ? String(it.icon.source) : ""
            return src !== "" ? "[icon " + src.replace(/^.*\//, "") + "]" : ""
        }
        if (_isImage(it))
            return String(it.source).replace(/^.*\//, "").substring(0, 60)
        return ""
    }

    function _fontPx(it) {
        if (!it.font)
            return 0
        return it.font.pixelSize > 0 ? it.font.pixelSize : it.font.pointSize * 4 / 3
    }

    /// Where a Text's glyphs actually are, in window coordinates.
    function _textBox(n) {
        var it = n.item
        var lp = it.leftPadding || 0, rp = it.rightPadding || 0, tp = it.topPadding || 0, bp = it.bottomPadding || 0
        var aw = it.width - lp - rp, ah = it.height - tp - bp
        var cw = it.contentWidth, ch = it.contentHeight
        var x = lp, y = tp
        var ha = it.effectiveHorizontalAlignment !== undefined ? it.effectiveHorizontalAlignment : it.horizontalAlignment
        if (ha === Text.AlignRight) x = lp + aw - cw
        else if (ha === Text.AlignHCenter) x = lp + (aw - cw) / 2
        if (it.verticalAlignment === Text.AlignBottom) y = tp + ah - ch
        else if (it.verticalAlignment === Text.AlignVCenter) y = tp + (ah - ch) / 2
        var m = it.mapToItem(null, x, y, cw, ch)
        return _rect(m.x, m.y, m.width, m.height)
    }

    function _lum(c) {
        function ch(v) { return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4) }
        return 0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
    }
    function _blend(top, bottom) {
        var a = top.a
        return { r: top.r * a + bottom.r * (1 - a), g: top.g * a + bottom.g * (1 - a), b: top.b * a + bottom.b * (1 - a), a: 1 }
    }
    function _hex(c) {
        function h(v) { var s = Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16); return s.length < 2 ? "0" + s : s }
        return "#" + h(c.r) + h(c.g) + h(c.b)
    }

    /// The colour under (px, py) below node `idx`, composited from the
    /// rectangles painted before it, or null when an image or gradient is
    /// there and the colour cannot be known.
    function _backgroundAt(nodes, idx, px, py) {
        var layers = []
        for (var k = idx - 1; k >= 0; k--) {
            var b = nodes[k]
            var v = b.vis
            if (px < v.x || py < v.y || px >= v.x + v.w || py >= v.y + v.h)
                continue
            var it = b.item
            if (_isImage(it) && String(it.source) !== "" && b.opacity > 0.3 && v.w > 24 && v.h > 24)
                return null
            var c = null
            if (_isRect(it)) {
                if (it.gradient)
                    return null
                c = it.color
            } else if (it.fillColor !== undefined && it.fillColor !== null && it.fillColor.a !== undefined) {
                // A painted shape that names its fill (SidebarToggle's
                // hexagons); its bounding box stands in for the outline.
                c = it.fillColor
            } else if (typeof it.requestPaint === "function" || it.fragmentShader !== undefined
                       || _typeOf(it).name === "Shape") {
                // Something is painted here that the audit cannot read.
                if (b.opacity > 0.3 && v.w > 8 && v.h > 8)
                    return null
                continue
            } else {
                continue
            }
            var a = c.a * b.opacity
            if (a < 0.02)
                continue
            layers.push({ r: c.r, g: c.g, b: c.b, a: a, src: b })
            if (a >= 0.995)
                break
        }
        var wc = app.color
        var base = { r: wc.r, g: wc.g, b: wc.b, a: 1 }
        for (var i = layers.length - 1; i >= 0; i--)
            base = _blend(layers[i], base)
        // Named in the finding, so a wrong background can be told apart from
        // a wrong text colour.
        base.src = layers.length > 0 ? _path(layers[0].src) : "window"
        return base
    }

    function _isAncestor(a, b) {
        for (var n = b.parent; n; n = n.parent)
            if (n === a)
                return true
        return false
    }

    function _auditPage(scope, shot, page) {
        if (!scope) {
            _error("no scope item for " + (_scene ? _scene.name : "?"))
            return
        }
        var nodes = _walk(scope)
        var rec = { page: page, shot: shot, issues: [] }
        _scene.pages.push(rec)
        if (probe !== "") {
            var pt = probe.split(",").map(Number)
            rec.probe = []
            for (var q = nodes.length - 1; q >= 0 && rec.probe.length < 12; q--) {
                var pv = nodes[q].vis
                if (pt[0] >= pv.x && pt[1] >= pv.y && pt[0] < pv.x + pv.w && pt[1] < pv.y + pv.h) {
                    var pi = nodes[q].item
                    rec.probe.push(_path(nodes[q]) + (pi.color !== undefined && pi.color !== null && pi.color.a !== undefined
                        ? " color " + _hex(pi.color) + " a=" + Math.round(pi.color.a * 100) / 100 : "")
                        + " opacity " + Math.round(nodes[q].opacity * 100) / 100)
                }
            }
            rec.probe.push("window color " + _hex(app.color))
        }
        var seen = {}
        function add(node, rule, severity, message, rect) {
            var issue = {
                rule: rule, severity: severity, message: message,
                path: audit._path(node), text: audit._label(node.item),
                rect: audit._round(rect || node.vis)
            }
            var key = rule + "|" + issue.path + "|" + issue.text + "|" + issue.rect.x + "," + issue.rect.y
            if (seen[key])
                return
            seen[key] = true
            issue.allowed = audit._allowedReason(issue)
            rec.issues.push(issue)
        }

        // A dialog or popup does not clip, so content too big for it draws
        // over the dimmed page instead of being cut off.
        var scopeRect = null
        if (scope !== app.contentItem) {
            var sm = scope.mapToItem(null, 0, 0, scope.width, scope.height)
            scopeRect = _rect(sm.x, sm.y, sm.width, sm.height)
        }
        var escapes = function(node, box) {
            if (scopeRect && !audit._contains(scopeRect, box, 1.5))
                add(node, "escapes-dialog", "error", "Draws " + Math.round(Math.max(scopeRect.x - box.x, box.x + box.w - scopeRect.x - scopeRect.w,
                    scopeRect.y - box.y, box.y + box.h - scopeRect.y - scopeRect.h)) + " px outside its dialog.", box)
        }

        var overlapCandidates = []
        for (var i = 0; i < nodes.length; i++) {
            var n = nodes[i]
            if (!n.inScope || n.vis.w < 1 || n.vis.h < 1)
                continue
            var it = n.item
            var type = _typeOf(it)

            // A custom component built on one of these reports its own name
            // (IconButton, SquareSwitch), so only direct use matches.
            if (_roundControls.indexOf(type.name) >= 0)
                add(n, "round-control", "error",
                    "Material " + type.name + " used directly; the desktop uses the square stand-ins (IconButton, SquareSwitch, SquareSlider, SquareBusyIndicator).")

            if (_isRect(it) && it.radius > 0.5 && Math.min(n.rect.w, n.rect.h) >= 4
                    && (it.color.a > 0.02 || it.border.width > 0))
                add(n, "rounded-corner", "error", "Rectangle has radius " + Math.round(it.radius * 10) / 10
                    + "; the desktop is angular throughout.")

            if (_isText(it)) {
                var txt = _plain(it.text)
                if (txt === "")
                    continue
                // Fully transparent text is not drawn (StyledTextField hides
                // Material's floating placeholder this way); it has no
                // contrast, size or position on screen to check.
                if (it.color.a * n.opacity < 0.05)
                    continue
                var px = _fontPx(it)
                if (n.rect.w < 1 || n.rect.h < 1) {
                    add(n, "text-zero-size", "error", "Text has no room to draw (" + Math.round(n.rect.w) + "×" + Math.round(n.rect.h) + ").", n.rect)
                    continue
                }
                // Overflow is measured against the whole box: glyphs that only
                // eat into the padding still draw inside the item.
                var aw = it.width - (it.leftPadding || 0) - (it.rightPadding || 0)
                var box = _textBox(n)
                if (it.contentWidth > it.width + 1.5 && it.elide === Text.ElideNone)
                    add(n, "text-overflow", "error", "Text is " + Math.round(it.contentWidth - it.width) + " px wider than its box ("
                        + Math.round(it.contentWidth) + " in " + Math.round(it.width) + ")"
                        + (it.wrapMode !== Text.NoWrap ? "; a word longer than the line cannot wrap." : "."), box)
                if (it.contentHeight > it.height + 1.5 && !it.truncated && it.height > 0)
                    add(n, "text-overflow-v", "error", "Text is " + Math.round(it.contentHeight - it.height) + " px taller than its box ("
                        + Math.round(it.contentHeight) + " in " + Math.round(it.height) + ").", box)
                if (it.truncated)
                    add(n, "text-elided", "warn", "Text is cut short: \u201c" + txt.substring(0, 140) + "\u201d", box)
                if (it.wrapMode !== Text.NoWrap && it.lineCount >= 3 && aw < px * 5)
                    add(n, "text-squeezed", "error", "Wrapped text is squeezed into " + Math.round(aw) + " px (" + it.lineCount + " lines).", box)
                if (px > 0 && px < 10 && Theme.fontScale >= 1)
                    add(n, "text-tiny", "warn", "Text is " + Math.round(px * 10) / 10 + " px at the designed size or larger.", box)

                var visBox = _inter(box, n.clipAll)
                if (_area(visBox) > 0)
                    escapes(n, visBox)
                if (!_contains(n.clipStatic, box, 1.5) && _area(visBox) > 0) {
                    var root = _rootItem()
                    var offWindow = !_contains(_rect(0, 0, root.width, root.height), box, 1.5)
                    add(n, offWindow ? "offscreen" : "clipped", "error",
                        offWindow ? "Text runs past the edge of the window." : "Text is cut off by a clipping container.", box)
                }

                if (it.enabled && n.opacity >= 0.15 && _area(visBox) > 0) {
                    var bg = _backgroundAt(nodes, n.paint, visBox.x + visBox.w / 2, visBox.y + visBox.h / 2)
                    if (bg) {
                        var c = it.color
                        var fg = _blend({ r: c.r, g: c.g, b: c.b, a: c.a * n.opacity }, bg)
                        var l1 = _lum(fg), l2 = _lum(bg)
                        var ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
                        var large = px >= 24 || (px >= 18.66 && it.font.bold)
                        var need = large ? 3 : 4.5
                        if (ratio < need)
                            add(n, "contrast", ratio < 3 ? "error" : "warn",
                                "Contrast " + Math.round(ratio * 100) / 100 + ":1 (needs " + need + ":1): "
                                + _hex(fg) + " on " + _hex(bg) + " (" + bg.src + ").", visBox)
                    }
                }
                if (_area(visBox) > 0 && n.opacity >= 0.3)
                    overlapCandidates.push({ node: n, box: visBox, kind: "text" })
                continue
            }

            if (_isTextInput(it)) {
                if (it.contentHeight > it.height + 2 && it.height > 0)
                    add(n, "input-clipped", "error", "Input text is " + Math.round(it.contentHeight) + " px tall in a "
                        + Math.round(it.height) + " px field.")
            }

            // A MouseArea that takes no left click (a hover detector, a
            // right-click menu catcher behind a bubble) is not something to
            // aim at, so it is not held to a target size.
            var clickable = (_isButton(it) || (_isMouseArea(it) && (it.acceptedButtons & Qt.LeftButton) !== 0)) && it.enabled
            if (clickable) {
                escapes(n, n.vis)
                // A control's hit area is often larger than what it draws
                // (IconButton's tile), so this is a warning with some slack;
                // cut-off text is caught above as an error.
                if (!_contains(n.clipStatic, n.rect, 4) && _area(n.vis) > 0)
                    add(n, "clipped", "warn", "Control's clickable area is cut off" + (_contains(_rect(0, 0, _rootItem().width, _rootItem().height), n.rect, 4)
                        ? " by a clipping container." : " by the window edge."), n.rect)
                var side = Math.min(n.rect.w, n.rect.h)
                if (side < 24 && side > 0)
                    add(n, "small-target", "warn", "Clickable area is " + Math.round(n.rect.w) + "×" + Math.round(n.rect.h)
                        + " px; under 24 px is hard to hit.", n.rect)
            }

            if (_isButton(it) && it.enabled) {
                var hasText = _plain(it.text) !== ""
                var acc = "", tip = ""
                try { acc = it.Accessible.name || "" } catch (e1) {}
                try { tip = it.ToolTip.text || "" } catch (e2) {}
                if (!hasText && acc === "" && tip === "") {
                    var iconOnly = it.icon && String(it.icon.source || "") !== ""
                    add(n, "unlabeled-control", "warn", iconOnly
                        ? "Icon-only button has no tooltip or accessible name."
                        : (it.checkable ? "Toggle has no text or accessible name; a screen reader announces it unnamed."
                                        : "Button has no text, tooltip or accessible name."))
                }
            }

            if (_isFlickable(it) && it.interactive && it.contentWidth > it.width + 2
                    && it.flickableDirection !== Flickable.VerticalFlick && it.orientation !== ListView.Horizontal
                    && it.width > 120)
                add(n, "horizontal-scroll", "warn", "Content is " + Math.round(it.contentWidth - it.width)
                    + " px wider than this " + type.name + ", so it scrolls sideways.")

            if (_isImage(it) && String(it.source) !== "" && n.vis.w >= 6 && n.vis.h >= 6 && n.opacity >= 0.3)
                overlapCandidates.push({ node: n, box: n.vis, kind: "image" })
        }

        // Partial overlaps only: one box wholly inside another is layering on
        // purpose (initials on an avatar, a label on an image).
        for (var a = 0; a < overlapCandidates.length; a++) {
            for (var b = a + 1; b < overlapCandidates.length; b++) {
                var A = overlapCandidates[a], B = overlapCandidates[b]
                if (A.kind === "image" && B.kind === "image")
                    continue
                // A floating control over scrolled content is meant to cover it.
                if (A.node.scroller !== B.node.scroller)
                    continue
                var x = _inter(A.box, B.box)
                // Line boxes carry leading: a few pixels of overlap between a
                // label and the line below it is not a visible collision.
                if (x.w < 4 || x.h < Math.max(4, 0.3 * Math.min(A.box.h, B.box.h)))
                    continue
                if (_contains(A.box, B.box, 1) || _contains(B.box, A.box, 1))
                    continue
                if (_isAncestor(A.node, B.node) || _isAncestor(B.node, A.node))
                    continue
                var other = _label(A.node.item)
                add(B.node, "overlap", "error", "Overlaps " + A.kind + " \u201c" + other.substring(0, 60) + "\u201d by "
                    + Math.round(x.w) + "×" + Math.round(x.h) + " px.", _rect(Math.min(A.box.x, B.box.x), Math.min(A.box.y, B.box.y),
                    Math.max(A.box.x + A.box.w, B.box.x + B.box.w) - Math.min(A.box.x, B.box.x),
                    Math.max(A.box.y + A.box.h, B.box.y + B.box.h) - Math.min(A.box.y, B.box.y)))
            }
        }
    }

    // ── Allowlist ───────────────────────────────────────────────────────────

    function _fileUrl(path) {
        var p = String(path).replace(/\\/g, "/")
        if (p.indexOf("/") !== 0)
            p = "/" + p
        return "file://" + encodeURI(p)
    }

    function _loadAllow() {
        if (allowPath === "")
            return
        var xhr = new XMLHttpRequest()
        xhr.onreadystatechange = function() {
            if (xhr.readyState !== XMLHttpRequest.DONE)
                return
            try {
                var list = JSON.parse(xhr.responseText)
                audit._allow = Array.isArray(list) ? list : (list.allow || [])
            } catch (e) {
                audit._error("allowlist " + audit.allowPath + " is not valid JSON: " + e)
            }
        }
        xhr.open("GET", _fileUrl(allowPath))
        xhr.send()
    }

    function _allowedReason(issue) {
        for (var i = 0; i < _allow.length; i++) {
            var a = _allow[i]
            try {
                if (a.rule && a.rule !== issue.rule) continue
                if (a.path && !new RegExp(a.path).test(issue.path)) continue
                if (a.text && !new RegExp(a.text).test(issue.text)) continue
                if (a.variant && !(new RegExp(a.variant).test(_variant ? _variant.id : ""))) continue
                return a.reason || "allowed"
            } catch (e) {}
        }
        return ""
    }

    // ── Report ─────────────────────────────────────────────────────────────

    function _summarize() {
        var bySeverity = { error: 0, warn: 0, allowed: 0 }
        var byRule = {}
        var unique = {}
        for (var v = 0; v < report.variants.length; v++) {
            var variant = report.variants[v]
            for (var s = 0; s < variant.scenes.length; s++) {
                var scene = variant.scenes[s]
                for (var p = 0; p < scene.pages.length; p++) {
                    var page = scene.pages[p]
                    for (var i = 0; i < page.issues.length; i++) {
                        var is = page.issues[i]
                        var key = is.rule + "|" + is.path + "|" + is.text
                        var where = variant.id + " / " + scene.name + (page.page > 0 ? " p" + (page.page + 1) : "")
                        if (!unique[key]) {
                            unique[key] = { rule: is.rule, severity: is.severity, path: is.path, text: is.text,
                                            message: is.message, worst: where, allowed: is.allowed, where: [],
                                            shot: page.shot, rect: is.rect }
                            if (is.allowed) bySeverity.allowed++
                            else bySeverity[is.severity] = (bySeverity[is.severity] || 0) + 1
                            byRule[is.rule] = (byRule[is.rule] || 0) + 1
                        } else if (is.severity === "error" && unique[key].severity !== "error") {
                            // The worst occurrence decides the finding's
                            // severity, and its message and place are the
                            // ones reported.
                            if (!unique[key].allowed) { bySeverity[unique[key].severity]--; bySeverity.error++ }
                            unique[key].severity = "error"
                            unique[key].message = is.message
                            unique[key].worst = where
                            unique[key].shot = page.shot
                            unique[key].rect = is.rect
                        }
                        if (unique[key].where.indexOf(where) < 0)
                            unique[key].where.push(where)
                    }
                }
            }
        }
        var list = []
        for (var k in unique)
            list.push(unique[k])
        var rank = { error: 0, warn: 1 }
        list.sort(function(x, y) {
            if (!!x.allowed !== !!y.allowed) return x.allowed ? 1 : -1
            if (rank[x.severity] !== rank[y.severity]) return rank[x.severity] - rank[y.severity]
            return x.rule < y.rule ? -1 : x.rule > y.rule ? 1 : (x.path < y.path ? -1 : 1)
        })
        report.summary = { errors: bySeverity.error, warnings: bySeverity.warn, allowed: bySeverity.allowed,
                           byRule: byRule, unique: list }
    }

    function _put(name, text, done) {
        var xhr = new XMLHttpRequest()
        xhr.onreadystatechange = function() {
            if (xhr.readyState === XMLHttpRequest.DONE)
                done()
        }
        xhr.open("PUT", _fileUrl(outDir + "/" + name))
        xhr.send(text)
    }

    function _finish() {
        if (_finished)
            return
        _finished = true
        stepTimer.stop()
        asyncWatchdog.stop()
        report.finished = new Date().toISOString()
        report.variants = report.variants.filter(function(v) { return v.scenes.length > 0 })
        _summarize()
        var json = JSON.stringify(report, null, 1)
        audit._put("report.json", json, function() {
            audit._put("report.html", audit._html(json), function() {
                console.log("[ui-audit] done: " + report.summary.errors + " errors, " + report.summary.warnings
                    + " warnings, " + report.summary.allowed + " allowed")
                Qt.quit()
            })
        })
    }

    function _html(json) {
        var safe = json.replace(/</g, "\\u003c")
        return "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">"
            + "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
            + "<title>UI audit</title><style>" + _css + "</style></head><body>"
            + "<header><h1>UI audit</h1><p id=\"meta\"></p><div id=\"counts\"></div>"
            + "<div class=\"filters\"><label><input type=\"checkbox\" id=\"f-error\" checked> Errors</label>"
            + "<label><input type=\"checkbox\" id=\"f-warn\" checked> Warnings</label>"
            + "<label><input type=\"checkbox\" id=\"f-allowed\"> Allowed</label>"
            + "<select id=\"f-rule\"><option value=\"\">All rules</option></select>"
            + "<input type=\"search\" id=\"f-text\" placeholder=\"Filter by path or text\"></div></header>"
            + "<main><section><h2>Findings</h2><p class=\"hint\">Each finding appears once however many screens "
            + "and variants show it; select one to see where.</p><table id=\"unique\"><thead><tr><th>Severity</th><th>Rule</th>"
            + "<th>Finding</th><th>Where</th></tr></thead><tbody></tbody></table></section>"
            + "<section><h2>Screens</h2><div id=\"screens\"></div></section>"
            + "<!--QML_WARNINGS--></main>"
            + "<script type=\"application/json\" id=\"data\">" + safe + "</script>"
            + "<script>" + _js + "</script></body></html>\n"
    }

    readonly property string _css: [
        ":root{--bg:#f6f7f9;--panel:#fff;--text:#1d2026;--muted:#5d6470;--line:#d9dde3;--err:#c4162a;--warn:#a15c00;--ok:#2a7a46;--accent:#3b4bd8}",
        "@media (prefers-color-scheme:dark){:root{--bg:#111214;--panel:#1e1f22;--text:#dcddde;--muted:#a6a9b0;--line:#383a40;--err:#ff6b7a;--warn:#f0b04a;--ok:#5cc98a;--accent:#8c9bff}}",
        "*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:14px/1.45 system-ui,Segoe UI,sans-serif}",
        "header{padding:16px 24px;border-bottom:1px solid var(--line);background:var(--panel);position:sticky;top:0;z-index:5}",
        "h1{margin:0 0 4px;font-size:20px}h2{font-size:16px;margin:24px 0 8px}h3{font-size:14px;margin:16px 0 6px}",
        "main{padding:0 24px 48px;max-width:1500px}.hint,#meta{color:var(--muted);margin:0 0 8px}",
        "#counts span{display:inline-block;margin-right:16px;font-weight:600}.e{color:var(--err)}.w{color:var(--warn)}.a{color:var(--muted)}",
        ".filters{display:flex;flex-wrap:wrap;gap:12px;align-items:center;margin-top:8px}",
        "select,input[type=search]{background:var(--bg);color:var(--text);border:1px solid var(--line);padding:4px 6px;font:inherit}",
        "table{border-collapse:collapse;width:100%;background:var(--panel)}th,td{border:1px solid var(--line);padding:6px 8px;text-align:left;vertical-align:top}",
        "tbody tr{cursor:pointer}tbody tr:hover{background:var(--bg)}tr.sel{outline:2px solid var(--accent)}",
        "code{font:12px/1.4 ui-monospace,Consolas,monospace;word-break:break-word}td.where{font-size:12px;color:var(--muted);max-width:360px}",
        ".variant{margin-top:24px;border-top:1px solid var(--line)}.page{display:flex;flex-wrap:wrap;gap:16px;margin:8px 0 24px}",
        ".shot{position:relative;flex:none;border:1px solid var(--line);max-width:100%;overflow:auto}.shot img{display:block}",
        ".box{position:absolute;border:2px solid var(--err);background:rgba(255,0,40,.08);pointer-events:auto}",
        ".box.warn{border-color:var(--warn);background:rgba(240,160,0,.08)}.box.allowed{border-style:dashed;border-color:var(--muted);background:none}",
        ".box.hl{border-width:3px;box-shadow:0 0 0 3px var(--accent)}",
        ".issues{flex:1;min-width:280px;max-width:520px;margin:0;padding:0;list-style:none}.issues li{padding:6px 8px;border-bottom:1px solid var(--line);cursor:pointer}",
        ".issues li:hover,.issues li.hl{background:var(--panel)}.sev{font-weight:700;text-transform:uppercase;font-size:11px}",
        "pre{white-space:pre-wrap;background:var(--panel);border:1px solid var(--line);padding:8px;font:12px/1.4 ui-monospace,Consolas,monospace}"
    ].join("\n")

    readonly property string _js: [
        "const R=JSON.parse(document.getElementById('data').textContent);",
        "const $=s=>document.querySelector(s);const esc=s=>String(s==null?'':s).replace(/[&<>\"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','\"':'&quot;'}[c]));",
        "$('#meta').textContent=`Version ${R.version} on ${R.platform}, ${R.matrix} matrix, ${R.variants.length} variants; ${R.started} to ${R.finished}`+(R.errors.length?` - ${R.errors.length} run errors (see below)`:'');",
        "const S=R.summary;$('#counts').innerHTML=`<span class=e>${S.errors} errors</span><span class=w>${S.warnings} warnings</span><span class=a>${S.allowed} allowed</span>`;",
        "Object.keys(S.byRule).sort().forEach(r=>{const o=document.createElement('option');o.value=r;o.textContent=`${r} (${S.byRule[r]})`;$('#f-rule').appendChild(o)});",
        "function keep(i){const f=$('#f-text').value.toLowerCase();if(i.allowed&&!$('#f-allowed').checked)return false;",
        " if(!i.allowed&&!$('#f-'+i.severity).checked)return false;if($('#f-rule').value&&i.rule!==$('#f-rule').value)return false;",
        " return !f||(i.path+' '+i.text+' '+i.message).toLowerCase().includes(f)}",
        "function sevLabel(i){return i.allowed?'<span class=\"sev a\">allowed</span>':`<span class=\"sev ${i.severity==='error'?'e':'w'}\">${i.severity}</span>`}",
        "function renderUnique(){const tb=$('#unique tbody');tb.innerHTML='';S.unique.filter(keep).forEach(u=>{const tr=document.createElement('tr');",
        " tr.innerHTML=`<td>${sevLabel(u)}</td><td><code>${esc(u.rule)}</code></td><td>${esc(u.message)}${u.allowed?`<br><em class=a>${esc(u.allowed)}</em>`:''}<br><code>${esc(u.path)}</code>${u.text?`<br>“${esc(u.text)}”`:''}</td><td class=where><b>worst: ${esc(u.worst)}</b><br>${u.where.map(esc).join('<br>')}</td>`;",
        " tr.onclick=()=>{const k=CSS.escape(u.rule+'|'+u.path+'|'+u.text);const b=document.querySelector(`.box[data-key=\"${k}\"][data-shot=\"${CSS.escape(u.shot)}\"]`)||document.querySelector(`.box[data-key=\"${k}\"]`);if(b){document.querySelectorAll('.hl').forEach(x=>x.classList.remove('hl'));b.classList.add('hl');b.scrollIntoView({block:'center'})}};tb.appendChild(tr)})}",
        "function renderScreens(){const root=$('#screens');root.innerHTML='';R.variants.forEach(v=>{const vd=document.createElement('div');vd.className='variant';",
        " vd.innerHTML=`<h2>${esc(v.label)}</h2><p class=hint>Theme ${esc(v.theme)}, skin ${esc(v.skin)}, text ${v.font>=0?'+':''}${v.font}%, window ${v.size.w}×${v.size.h}</p>`;",
        " v.scenes.forEach(sc=>sc.pages.forEach(pg=>{const issues=pg.issues.filter(keep);const h=document.createElement('h3');h.textContent=sc.name+(pg.page>0?` (page ${pg.page+1})`:'')+` - ${issues.length} shown`;vd.appendChild(h);",
        "  const row=document.createElement('div');row.className='page';const shot=document.createElement('div');shot.className='shot';",
        "  const img=new Image();img.src=pg.shot;img.loading='lazy';img.onload=()=>{img.style.width=(v.size.w||img.naturalWidth)+'px'};shot.appendChild(img);",
        "  const ul=document.createElement('ul');ul.className='issues';issues.forEach(i=>{const b=document.createElement('div');b.className='box '+(i.allowed?'allowed':i.severity);",
        "   b.dataset.key=i.rule+'|'+i.path+'|'+i.text;b.dataset.shot=pg.shot;Object.assign(b.style,{left:i.rect.x+'px',top:i.rect.y+'px',width:Math.max(4,i.rect.w)+'px',height:Math.max(4,i.rect.h)+'px'});b.title=i.rule+': '+i.message;shot.appendChild(b);",
        "   const li=document.createElement('li');li.innerHTML=`${sevLabel(i)} <code>${esc(i.rule)}</code> ${esc(i.message)}<br><code>${esc(i.path)}</code>`;",
        "   const on=()=>{b.classList.add('hl');li.classList.add('hl')},off=()=>{b.classList.remove('hl');li.classList.remove('hl')};li.onmouseenter=on;li.onmouseleave=off;b.onmouseenter=on;b.onmouseleave=off;ul.appendChild(li)});",
        "  row.appendChild(shot);row.appendChild(ul);vd.appendChild(row)}));root.appendChild(vd)});",
        " if(R.errors.length){const d=document.createElement('div');d.innerHTML='<h2>Run errors</h2><pre>'+R.errors.map(e=>esc(`[${e.variant} / ${e.scene}] ${e.message}`)).join('\\n')+'</pre>';root.appendChild(d)}}",
        "function render(){renderUnique();renderScreens()}['#f-error','#f-warn','#f-allowed','#f-rule','#f-text'].forEach(s=>$(s).addEventListener('input',render));render();"
    ].join("\n")
}
