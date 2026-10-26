import XCTest

extension EditorLaunchTests {
    @MainActor func testIPadCompatibilityJourneys() throws {
        continueAfterFailure = true
        try checkIPadJourneys([
            ("Canvas Size dark", testCanvasSize), ("Canvas Size light", testCanvasSizeLight),
            ("Image Size dark", testImageSize), ("Image Size light", testImageSizeLight),
            ("Editor light", testCompleteEditorCapture), ("Editor dark", testCompleteEditorDarkCapture),
        ])
        try testIPadPanelCompatibilityJourneys()
    }
    @MainActor func testIPadPanelCompatibilityJourneys() throws {
        try testIPadColorCompatibilityJourneys()
        try testIPadPanelLayoutCompatibilityJourneys()
    }
    @MainActor func testIPadColorCompatibilityJourneys() throws {
        continueAfterFailure = true
        try checkIPadJourneys([
            ("Color light", testEditColor), ("Color dark", testEditColorDark),
        ])
    }
    @MainActor func testIPadPanelLayoutCompatibilityJourneys() throws {
        continueAfterFailure = true
        try checkIPadJourneys([
            ("Layer scrolling", testLayerListScrolling),
            ("Layer properties light", testLayerPropertiesDrawer), ("Layer properties dark", testLayerPropertiesDrawerDark),
            ("Drawing tabs light", testNativeDrawingTabs), ("Drawing tabs dark", testNativeDrawingTabsDark),
            ("Workspace switcher", testWorkspaceSwitcher),
        ])
    }
    @MainActor private func checkIPadJourneys(_ journeys: [(String, () throws -> Void)]) throws {
        for (name, journey) in journeys {
            try XCTContext.runActivity(named: name) { _ in try journey() }
            editorTestApplication().terminate()
        }
    }
    @MainActor func testOwnerWorkloadShaderFirstUseBothThemes() throws {
        continueAfterFailure = true
        for theme in ["light", "dark"] {
            try XCTContext.runActivity(named: "Owner painting and history " + theme) { _ in
                try checkOwnerWorkloadShaderFirstUse(theme: theme)
            }
            editorTestApplication().terminate()
        }
    }
    @MainActor func testOwnerWorkloadShaderFirstUse() throws { try checkOwnerWorkloadShaderFirstUse(theme: "light") }
    @MainActor func testOwnerWorkloadShaderFirstUseDark() throws { try checkOwnerWorkloadShaderFirstUse(theme: "dark") }

    @MainActor private func checkOwnerWorkloadShaderFirstUse(theme: String) throws {
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_WORKLOAD"] = "ink"
        app.launchEnvironment["CAPY_WORKLOAD_SECONDS"] = "180"
        app.launchEnvironment["CAPY_TRACE_GPU"] = "0"
        app.launchEnvironment["CAPY_GPU_RECOVERY_TEST"] = "1"
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        let launched = Date()
        app.launch(); waitForStartupCanvas(app)
        let status = app.staticTexts["renderer-test-status"]
        let point = CGPoint(x: 0.5, y: 0.55)
        func pixels() -> Data { editorPixels(in: app, at: point, size: 512) }
        func select(_ tool: String, _ group: String, _ preset: String) {
            selectPaintPreset(tool, group: group, preset: preset, in: app)
            editPaintSize("24", in: app)
            expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Renderer ready", "Renderer ready"), evaluatedWith: status)
            waitForExpectations(timeout: 30)
        }
        func rendered(_ preset: String) {
            XCTAssertLessThan(Date().timeIntervalSince(launched), 180, "The native owner producer must still be running")
            let before = pixels()
            let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in pixels() != before }, object: app)
            let rendered = XCTWaiter.wait(for: [changed], timeout: 15) == .completed
            attachEditor(in: app, name: "owner-workload-first-use-\(preset)-\(theme)")
            XCTAssertTrue(rendered, "\(preset) must render while existing native-owner samples are delivered")
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        }
        for (tool, group, preset) in [("Pen", "Pen", "G-Pen"), ("Paint Brush", "Paint", "Dry Scumble"),
            ("Paint Brush", "Watercolor", "Watercolor Wash"), ("Blend", "Paint mixing", "Natural Blender"),
            ("Liquify", "Liquify", "Liquify Push")] {
            select(tool, group, preset); rendered(preset)
        }
        let finished = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in Date().timeIntervalSince(launched) >= 225 }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [finished], timeout: 230), .completed)
        let ready = app.staticTexts["recovery-status"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Recovery ready", "Recovery ready"), evaluatedWith: ready)
        waitForExpectations(timeout: 30)
        let painted = pixels()
        XCTAssertTrue(stride(from: 0, to: painted.count, by: 4).contains {
            Int(painted[$0 + 2]) > Int(painted[$0]) + 20 && painted[$0 + 3] > 0
        }, "The paper crop must contain authored blue pigment")
        attachEditor(in: app, name: "owner-workload-before-clear-\(theme)")
        editorMenu(in: app, menu: "Layer", id: "clear_layer", label: "Clear Entire Layer")
        let changed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in pixels() != painted }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [changed], timeout: 15), .completed)
        let cleared = pixels()
        attachEditor(in: app, name: "owner-workload-before-undo-\(theme)")
        editorHistory("Undo", in: app)
        let undone = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in pixels() == painted }, object: app)
        let undoResult = XCTWaiter.wait(for: [undone], timeout: 15)
        attachEditor(in: app, name: "owner-workload-after-undo-\(theme)")
        XCTAssertEqual(undoResult, .completed)
        editorHistory("Redo", in: app)
        let redone = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in pixels() == cleared }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [redone], timeout: 15), .completed)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "owner-workload-first-use-history-\(theme)")
        let provenance = XCTAttachment(string: "Existing synthetic native-owner samples; ordinary UIKit tool selections at 24 px to preserve visible ink edges; noncanonical functional workload; no physical Pencil sensor or tier timing claim")
        provenance.name = "owner-workload-provenance"; provenance.lifetime = .keepAlways; add(provenance)
    }

    @MainActor func testNativeDrawingLifecycleStress() throws { try checkNativeDrawingLifecycleStress(theme: "light") }
    @MainActor func testNativeDrawingLifecycleStressDark() throws { try checkNativeDrawingLifecycleStress(theme: "dark") }

    @MainActor private func waitForStartupCanvas(_ app: XCUIApplication) {
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 60)
        waitForEditorInteraction(in: app)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor private func checkNativeDrawingLifecycleStress(theme: String) throws {
        guard let storage = ProcessInfo.processInfo.environment["CAPY_UI_DRAWING_STORAGE_" + theme.uppercased()] else {
            throw XCTSkip("Seed an owned Release G-Pen session into private app storage before this test")
        }
        XCTAssertTrue(storage.hasPrefix("capy-test-drawing-"))
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_STORAGE_DIR"] = storage
        app.launch(); waitForStartupCanvas(app)
        attachEditor(in: app, name: "startup-release-ink-before-controls-\(theme)")
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let camera = app.buttons["camera-status"]
        XCTAssertTrue(camera.waitForExistence(timeout: 10))
        let originalCamera = camera.label
        withHiddenChrome(in: app) { canvas.pinch(withScale: 1.4, velocity: 1) }
        expectation(for: NSPredicate { _, _ in camera.label != originalCamera }, evaluatedWith: camera)
        waitForExpectations(timeout: 15)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let sample = CGPoint(x: 0.3, y: 0.35)
        let painted = editorPixels(in: app, at: sample, size: 768)
        attachEditor(in: app, name: "startup-release-ink-restored-\(theme)")
        XCTAssertTrue(stride(from: 0, to: painted.count, by: 4).contains { Int(painted[$0 + 2]) > Int(painted[$0]) + 20 })
        editorMenu(in: app, menu: "Edit", id: "undo", label: "Undo")
        let undone = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 768) != painted }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [undone], timeout: 15), .completed)
        let previous = editorPixels(in: app, at: sample, size: 768)
        editorMenu(in: app, menu: "Edit", id: "redo", label: "Redo")
        let redone = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 768) == painted }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [redone], timeout: 15), .completed)
        let scene = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-")).firstMatch
        let sceneID = scene.identifier
        XCTAssertFalse(sceneID.isEmpty)
        for cycle in 1...10 {
            XCUIDevice.shared.press(.home)
            let background = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                [.runningBackground, .runningBackgroundSuspended].contains(app.state)
            }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [background], timeout: 15), .completed)
            app.activate(); waitForStartupCanvas(app)
            XCTAssertEqual(scene.identifier, sceneID)
            XCTAssertEqual(editorPixels(in: app, at: sample, size: 768), painted, "Lifecycle cycle \(cycle) must preserve the real stroke")
        }
        func history() {
            editorMenu(in: app, menu: "Edit", id: "undo", label: "Undo")
            let undone = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 768) == previous }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [undone], timeout: 15), .completed)
            editorMenu(in: app, menu: "Edit", id: "redo", label: "Redo")
            let redone = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 768) == painted }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [redone], timeout: 15), .completed)
        }
        history()
        attachEditor(in: app, name: "startup-native-ten-lifecycle-cycles-\(theme)")
        app.terminate(); app.launch(); waitForStartupCanvas(app)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let restored = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 768) == painted }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [restored], timeout: 30), .completed)
        history()
        attachEditor(in: app, name: "startup-release-ink-reopened-\(theme)")
    }

    @MainActor func testMalformedRecoveryStillAllowsNewDrawing() throws { try checkMalformedRecoveryStillAllowsNewDrawing(theme: "light") }
    @MainActor func testMalformedRecoveryStillAllowsNewDrawingDark() throws { try checkMalformedRecoveryStillAllowsNewDrawing(theme: "dark") }

    @MainActor private func checkMalformedRecoveryStillAllowsNewDrawing(theme: String) throws {
        guard let storage = ProcessInfo.processInfo.environment["CAPY_UI_MALFORMED_STORAGE_" + theme.uppercased()] else {
            throw XCTSkip("Seed an unreadable head into private app storage before this test")
        }
        XCTAssertTrue(storage.hasPrefix("capy-test-malformed-"))
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_STORAGE_DIR"] = storage
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch()
        let retry = app.buttons["recovery-retry"], later = app.buttons["recovery-later"]
        XCTAssertTrue(retry.waitForExistence(timeout: 60)); XCTAssertTrue(later.isHittable)
        waitForStartupCanvas(app)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: retry)
        waitForExpectations(timeout: 60)
        workspaceActivate(retry)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: retry)
        waitForExpectations(timeout: 60)
        XCTAssertTrue(later.isHittable)
        editorMenu(in: app, menu: "File", id: "new_document", label: "New drawing")
        let create = app.buttons["new-drawing-create"]
        XCTAssertTrue(create.waitForExistence(timeout: 10)); workspaceActivate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 20)); waitForStartupCanvas(app)
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let count = rows.count
        workspaceActivate(app.buttons["layer-New layer"])
        expectation(for: NSPredicate { _, _ in rows.count == count + 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(later.isHittable)
        workspaceActivate(later)
        XCTAssertTrue(later.waitForNonExistence(timeout: 10))
        XCUIDevice.shared.press(.home); app.activate(); waitForStartupCanvas(app)
        XCTAssertEqual(rows.count, count + 1)
        attachEditor(in: app, name: "malformed-recovery-new-drawing-usable-" + theme)
    }
}

extension EditorLaunchTests {
    @MainActor func testSettingsTextSelectionShortcut() {
        checkSettingsTextState(in: editorCaptureApplication())
    }

    @MainActor func testTwoFingerCanvasNavigation() throws {
        try checkTwoFingerCanvasNavigation(theme: "light")
    }

    @MainActor func testTwoFingerCanvasNavigationDark() throws {
        try checkTwoFingerCanvasNavigation(theme: "dark")
    }

    @MainActor private func checkTwoFingerCanvasNavigation(theme: String) throws {
        let app = editorCaptureApplication()
        let panels = ["toolbar", "commands", "brushes", "tool_settings", "sizes", "color", "palettes",
            "stats", "navigator", "proof", "properties", "adjustments", "layers", "histogram", "waveform",
            "brush_sets", "sculpt_sets", "tools", "filter_types"]
        let actions: [[String: Any]] = [["type": "set_theme", "theme": theme]] + panels.map {
            ["type": "customize", "action": ["type": "set_panel_visible", "panel": $0, "visible": false]]
        }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        let status = app.buttons["camera-status"]
        func camera() -> (zoom: Int, rotation: Int) {
            let parts = status.label.components(separatedBy: " · ")
            guard parts.count == 2,
                let zoom = Int(parts[0].replacingOccurrences(of: "%", with: "")),
                let rotation = Int(parts[1].replacingOccurrences(of: "°", with: "")) else {
                XCTFail("Missing camera readout: \(status.label)"); return (0, 0)
            }
            return (zoom, rotation)
        }
        let original = camera()
        XCTAssertGreaterThan(original.zoom, 0)
        let sample = CGPoint(x: 0.5, y: 0.55)
        let paper = editorPixels(in: app, at: sample)
        XCTAssertTrue(paper.prefix(3).allSatisfy { $0 == 255 })

        // XCTest injects two actual UIKit contacts into the editor canvas.
        // These are native delivery checks, not physical finger/sensor evidence.
        withHiddenChrome(in: app) { canvas.pinch(withScale: 1.6, velocity: 1) }
        expectation(for: NSPredicate { _, _ in camera().zoom > original.zoom * 5 / 4 }, evaluatedWith: status)
        waitForExpectations(timeout: 10)
        let enlarged = camera()
        withHiddenChrome(in: app) { canvas.rotate(.pi / 6, withVelocity: .pi / 2) }
        expectation(for: NSPredicate { _, _ in abs(camera().rotation - enlarged.rotation) >= 20 }, evaluatedWith: status)
        waitForExpectations(timeout: 10)
        XCTAssertLessThanOrEqual(abs(camera().rotation - enlarged.rotation), 40)
        attachEditor(in: app, name: "two-finger-zoom-rotation")

        // A fresh pair must work after the preceding contacts have ended.
        let rotated = camera()
        withHiddenChrome(in: app) { canvas.pinch(withScale: 0.7, velocity: -1) }
        expectation(for: NSPredicate { _, _ in camera().zoom < rotated.zoom * 4 / 5 }, evaluatedWith: status)
        waitForExpectations(timeout: 10)
        XCTAssertEqual(viewport.frame, originalFrame, "Navigation must leave the OS window fixed")
        workspaceActivate(app.buttons["menu-Edit"])
        for command in ["undo", "redo"] {
            let button = app.buttons["command-" + command]
            XCTAssertTrue(button.waitForExistence(timeout: 5))
            XCTAssertFalse(button.isEnabled, "Two-finger navigation must not paint or create artwork history")
        }
        canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.55)).tap()
        editorMenu(in: app, menu: "View", id: "reset_view", label: "Reset view")
        expectation(for: NSPredicate { _, _ in
            let current = camera()
            return current.zoom == original.zoom && current.rotation == original.rotation
        }, evaluatedWith: status)
        waitForExpectations(timeout: 10)
        XCTAssertEqual(editorPixels(in: app, at: sample), paper)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "two-finger-view-restored")
    }

    @MainActor func testNativeWorkspaceContextAction() { checkNativeWorkspaceContextAction() }

    @MainActor func testLayerGripAndChildActions() {
        let app = editorTestApplication()
        app.launch()
        let add = app.buttons["layer-New layer"]
        XCTAssertTrue(add.waitForExistence(timeout: 20))
        add.tap()
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let addedID = rows.element(boundBy: 0).identifier
        let originalID = rows.element(boundBy: 1).identifier
        let original = app.otherElements[originalID]
        original.buttons["Select layer without changing drawing target"].tap()
        XCTAssertTrue(original.buttons["Select layer without changing drawing target"].isSelected)
        XCTAssertTrue(app.otherElements[addedID].buttons["Edit layer content"].isSelected)
        let grip = app.descendants(matching: .any)[addedID.replacingOccurrences(of: "layer-row-", with: "layer-grip-")].firstMatch
        grip.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.01,
            thenDragTo: original.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.85)))
        expectation(for: NSPredicate { _, _ in rows.element(boundBy: 0).identifier == originalID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        func command(_ label: String) {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch.tap()
        }
        command("Undo")
        expectation(for: NSPredicate { _, _ in rows.element(boundBy: 0).identifier == addedID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        command("Redo")
        expectation(for: NSPredicate { _, _ in rows.element(boundBy: 0).identifier == originalID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }

    @MainActor func testLayerMenuDragUpward() throws {
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_ROW_MENU_PROBE"] = "1"
        app.launchEnvironment["CAPY_LAYER_INPUT_PROBE"] = "1"
        let actions = (0..<10).map { _ in ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]] as [String: Any] }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let list = app.descendants(matching: .any)["layer-rows"].firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 20))
        func order() -> [String] { (list.value as? String ?? "").split(separator: ",").map(String.init) }
        expectation(for: NSPredicate { _, _ in order().count == 12 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let initial = order(), movedID = initial[6]
        let row = app.otherElements["layer-row-" + movedID]
        XCTAssertTrue(row.isHittable)
        let destination = list.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0)).withOffset(CGVector(dx: 0, dy: 8))
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.5)).press(forDuration: 0.8,
            thenDragTo: destination, withVelocity: .slow, thenHoldForDuration: 0.3)
        expectation(for: NSPredicate { _, _ in order().first == movedID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let moved = order()
        XCTAssertEqual(moved.filter { $0 != movedID }, initial.filter { $0 != movedID })
        func command(_ label: String) {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch.tap()
        }
        command("Undo")
        expectation(for: NSPredicate { _, _ in order() == initial }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        command("Redo")
        expectation(for: NSPredicate { _, _ in order() == moved }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }

    @MainActor func testLayerListScrolling() throws {
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_LAYER_INPUT_PROBE"] = "1"
        let actions = (0..<22).map { _ in ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]] as [String: Any] }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let list = app.descendants(matching: .any)["layer-rows"].firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 20))
        func order() -> [String] { (list.value as? String ?? "").split(separator: ",").map(String.init) }
        expectation(for: NSPredicate { _, _ in order().count == 24 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let initial = order(), movedID = initial[0]
        let first = app.descendants(matching: .any)["layer-grip-" + movedID].firstMatch
        let initialFrame = first.frame
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.8)).press(forDuration: 0.01,
            thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.2)))
        XCTAssertTrue(!first.exists || first.frame.maxY < initialFrame.maxY - 20,
            "Early touch movement must scroll the rows")
        XCTAssertEqual(order(), initial, "Early touch scrolling must preserve document order")
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.15)).press(forDuration: 0.01,
            thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.9)))
        XCTAssertTrue(first.waitForExistence(timeout: 5))
        XCTAssertTrue(first.isHittable, "The source grip must be back inside the viewport")
        let edge = list.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 1)).withOffset(CGVector(dx: 0, dy: -10))
        first.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.01,
            thenDragTo: edge, withVelocity: .slow, thenHoldForDuration: 1.2)
        expectation(for: NSPredicate { _, _ in order() != initial }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let moved = order()
        XCTAssertEqual(moved.filter { $0 != movedID }, initial.filter { $0 != movedID })
        XCTAssertGreaterThan(moved.firstIndex(of: movedID) ?? -1, Int(list.frame.height / 40),
            "Holding at the edge must move the source beyond the initial visible rows")
        func command(_ label: String) {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch.tap()
        }
        command("Undo")
        expectation(for: NSPredicate { _, _ in order() == initial }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        command("Redo")
        expectation(for: NSPredicate { _, _ in order() == moved }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }

    @MainActor func testLayerContextMenuAnchors() {
        let app = editorCaptureApplication()
        app.launch()
        XCTAssertTrue(app.buttons["layer-New layer"].waitForExistence(timeout: 20))
        checkLayerControls(in: app)
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let row = rows.element(boundBy: 1)
        row.buttons["Edit layer content"].press(forDuration: 0.6)
        let nativeMenuItem = app.buttons["menu-action-Organize"]
        XCTAssertTrue(nativeMenuItem.waitForExistence(timeout: 5))
        attachLayerMenu("layer-content-context-anchor")
        // Dismiss through the app canvas, then check the separate footer origin.
        app.otherElements["canvas"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        XCTAssertTrue(nativeMenuItem.waitForNonExistence(timeout: 5))
        app.buttons["layer-Layer actions"].tap()
        let menu = app.descendants(matching: .any)["layer-context-menu"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        attachLayerMenu("layer-footer-context-anchor")
    }

    @MainActor private func attachLayerMenu(_ name: String) {
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        capture.name = name; capture.lifetime = .keepAlways; add(capture)
    }

    @MainActor func testCanvasContactKeepsWindow() {
        let app = editorCaptureApplication()
        app.launch()
        workspaceActivate(app.buttons["workspace-switch-builtin:workspace:painter"])
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let before = app.frame
        let start = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6))
        start.press(forDuration: 0.01, thenDragTo: start.withOffset(CGVector(dx: 150, dy: -100)))
        XCTAssertEqual(app.frame, before, "A canvas contact must not move or resize the OS window")
    }

    @MainActor func testMetalLaunchAndEditorCapture() throws {
        let app = editorTestApplication()
        app.launch()
        let canvas = app.otherElements["canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        let ready = NSPredicate(format: "value == %@", "Canvas ready")
        expectation(for: ready, evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let zen = app.buttons["zen-button"]
        XCTAssertTrue(zen.exists)
        XCTAssertEqual(zen.frame.width, 36, accuracy: 1)
        XCTAssertEqual(zen.frame.height, 36, accuracy: 1)
        XCTAssertGreaterThan(app.frame.width, app.frame.height, "Landscape capture must use a settled landscape window")
        XCTAssertEqual(canvas.frame.width, app.frame.width, accuracy: 1)
        XCTAssertEqual(canvas.frame.height, app.frame.height, accuracy: 1)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-editor-initial"
        shot.lifetime = .keepAlways
        add(shot)
        checkLayerControls(in: app)
        let layers = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        layers.name = "ipad-layer-added"; layers.lifetime = .keepAlways; add(layers)
    }

    // Opt-in native provider acceptance; each run owns a new UUID-named folder.
    @MainActor private func nativeFilesTestFolder() throws -> String {
        guard let token = ProcessInfo.processInfo.environment["CAPY_FILE_TEST_TOKEN"] else {
            throw XCTSkip("Native Files acceptance requires an isolated CAPY_FILE_TEST_TOKEN")
        }
        _ = try XCTUnwrap(UUID(uuidString: token))
        return "Capy Files " + token
    }

    @MainActor func testNativeFilesProjectRoundTrip() async throws {
        try await checkNativeFilesProjectRoundTrip(theme: "light")
    }

    @MainActor func testNativeFilesProjectRoundTripDark() async throws {
        try await checkNativeFilesProjectRoundTrip(theme: "dark")
    }

    @MainActor private func checkNativeFilesProjectRoundTrip(theme: String) async throws {
        continueAfterFailure = false
        let base = try nativeFilesTestFolder()
        let folder = base + " " + theme
        let provider = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_PACKAGE_PROVIDER"])
        guard provider == "Capy Test Files " + String(base.dropFirst("Capy Files ".count)) else { XCTFail("Private runner provider mismatch"); return }
        let root = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent(folder, isDirectory: true)
        let app = editorTestApplication()
        let initialStorage = try XCTUnwrap(app.launchEnvironment["CAPY_STORAGE_DIR"])
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"add_layer"},{"type":"set_color","rgba":[0.1,0.3,0.9,1]},{"type":"customize","action":{"type":"set_panel_visible","panel":"layers","visible":true}}]"#
        func capture(_ name: String, pixels: Data? = nil) {
            if let pixels {
                let sample = XCTAttachment(data: pixels, uniformTypeIdentifier: "public.data")
                sample.name = name + "-" + theme + "-sample-rgba8-srgb"; sample.lifetime = .keepAlways; add(sample)
                attachEditor(in: app, name: name + "-" + theme + "-native-canvas")
            }
            let tree = XCTAttachment(string: app.debugDescription)
            tree.name = name + "-" + theme + "-hierarchy"; tree.lifetime = .keepAlways; add(tree)
            let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            shot.name = name + "-" + theme; shot.lifetime = .keepAlways; add(shot)
        }
        func waitUntil(_ condition: @escaping () -> Bool, timeout: TimeInterval = 30) async {
            let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: app)
            await fulfillment(of: [ready], timeout: timeout)
        }
        func command(_ id: String) async {
            let labels = ["open_document": "Open…", "save_document": "Save", "save_document_as": "Save As…", "export_document": "Export…"]
            guard let label = labels[id] else { XCTFail("Unknown file command: " + id); return }
            editorMenu(in: app, menu: "File", id: id, label: label)
        }
        func folderTitle(_ label: String) -> XCUIElement {
            app.otherElements["DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: " + label]
        }
        func location() {
            workspaceActivate(app.cells["DOC.sidebar.item.On My iPad"])
            XCTAssertTrue(folderTitle("On My iPad").waitForExistence(timeout: 10))
            let container = fileProviderRoot(provider, in: app)
            XCTAssertTrue(container.waitForExistence(timeout: 10)); workspaceActivate(container)
            XCTAssertTrue(folderTitle(provider).waitForExistence(timeout: 10))
        }
        func enterTestFolder() {
            location()
            workspaceActivate(app.cells[folder + ", Folder"])
            XCTAssertTrue(folderTitle(folder).waitForExistence(timeout: 10))
        }
        func expectTitle(_ expected: String) {
            let selected = selectedNewDrawingDocument(in: app, expectedTitle: expected)
            XCTAssertTrue(selected.label.contains(expected) || selected.value?.contains(expected) == true,
                "The selected drawing must retain \(expected): \(selected)")
        }
        func bytes(_ name: String) async throws -> Data {
            let url = root.appendingPathComponent(name)
            return try await Task.detached(priority: .utility) {
                let directory = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                guard directory.isDirectory == true, directory.isSymbolicLink != true else { throw CocoaError(.fileReadCorruptFile) }
                let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true,
                    let count = values.fileSize, (1...16 * 1024 * 1024).contains(count) else { throw CocoaError(.fileReadCorruptFile) }
                return try Data(contentsOf: url)
            }.value
        }
        func changedBytes(_ name: String, from previous: Data) async throws -> Data {
            let deadline = Date().addingTimeInterval(30)
            repeat {
                XCTAssertFalse(app.otherElements["Browse View (Picker)"].exists, "Cold Save must not open a new picker")
                XCTAssertFalse(app.navigationBars["FullDocumentManagerViewControllerNavigationBar"].exists)
                XCTAssertFalse(app.alerts.firstMatch.exists)
                if let saved = try? await bytes(name), saved != previous { return saved }
                try await Task.sleep(for: .milliseconds(100))
            } while Date() < deadline
            XCTFail("Cold Save must change the exact external file: " + name)
            throw CocoaError(.fileWriteUnknown)
        }
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let recovery = app.staticTexts["recovery-status"]
        let persistence = app.staticTexts["persistence-status"]
        func ready() async {
            await waitUntil({ canvas.value as? String == "Canvas ready" })
            await waitUntil({ persistence.value as? String == theme })
            await waitUntil({ recovery.label == "Recovery ready" || recovery.value as? String == "Recovery ready" })
            XCTAssertTrue(app.scrollViews["layer-rows"].waitForExistence(timeout: 10))
        }
        func cancelUnselectedOpen() async {
            await command("open_document")
            let picker = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"]
            XCTAssertTrue(picker.waitForExistence(timeout: 20))
            workspaceActivate(app.buttons["Cancel"].firstMatch)
            XCTAssertTrue(picker.waitForNonExistence(timeout: 20))
        }
        func restart(_ expected: String, count: Int, pixels: Data) async {
            await cancelUnselectedOpen()
            await ready()
            capture("project-before-cold-restart-" + expected)
            let storage = app.launchEnvironment["CAPY_STORAGE_DIR"]
            app.terminate()
            app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS")
            app.launch()
            XCTAssertEqual(app.launchEnvironment["CAPY_STORAGE_DIR"], storage)
            await ready()
            await waitUntil({ rows.count == count })
            expectTitle(expected)
            editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
            await waitUntil({ self.editorPixels(in: app) == pixels }, timeout: 15)
            capture("project-cold-restored-" + expected)
        }
        func openProject(_ name: String, count: Int, pixels: Data) async {
            await command("open_document")
            enterTestFolder()
            let file = fileProviderFile(name + ".capy", in: app).images.firstMatch
            XCTAssertTrue(file.waitForExistence(timeout: 10)); workspaceActivate(file)
            submitFileProviderSelection(in: app)
            await waitUntil({ rows.count == count })
            expectTitle(name)
            editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
            await waitUntil({ self.editorPixels(in: app) == pixels }, timeout: 15)
            XCTAssertFalse(app.alerts.firstMatch.exists)
        }
        app.launch()
        await ready()
        await waitUntil({ rows.count == 3 })
        XCTAssertFalse(selectedNewDrawingDocument(in: app).id.isEmpty)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        await waitUntil({ let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50 }, timeout: 15)
        capture("project-painted-before-fit", pixels: editorPixels(in: app))
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let painted = editorPixels(in: app)
        capture("project-painted-reference", pixels: painted)
        await command("save_document_as")
        let firstSave = documentExportControls(in: app)
        location()
        XCTAssertFalse(app.cells[folder + ", Folder"].exists, "Each native run must create a fresh test folder")
        let newFolder = app.buttons["New Folder"]
        if !newFolder.exists {
            let overflow = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"]
                .buttons["OverflowBarButtonItem"]
            workspaceActivate(overflow)
            capture("project-new-folder-overflow")
        }
        workspaceActivate(newFolder)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        app.typeKey("a", modifierFlags: .command)
        app.typeText(folder + "\n")
        XCTAssertTrue(folderTitle(folder).waitForExistence(timeout: 10))
        replaceEditorText(firstSave.filename, with: "RoundTrip", in: app)
        XCTAssertEqual(firstSave.filename.value as? String, "RoundTrip")
        workspaceActivate(firstSave.commit)
        XCTAssertTrue(firstSave.commit.waitForNonExistence(timeout: 20))
        XCTAssertEqual(app.state, .runningForeground)
        expectTitle("RoundTrip")
        XCTAssertEqual(rows.count, 3)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        let originallySaved = try await bytes("RoundTrip.capy")
        XCTAssertFalse(originallySaved.isEmpty)
        capture("project-saved")
        app.terminate()
        app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS")
        useFreshStorage(app)
        let persistentStorage = try XCTUnwrap(app.launchEnvironment["CAPY_STORAGE_DIR"])
        XCTAssertNotEqual(persistentStorage, initialStorage)
        let invocation = XCTAttachment(data: try JSONSerialization.data(withJSONObject: [
            "theme": theme, "folder": folder, "provider": provider,
            "initial_storage": initialStorage, "persistent_storage": persistentStorage], options: [.sortedKeys]), uniformTypeIdentifier: "public.json")
        invocation.name = "native-files-invocation-" + theme; invocation.lifetime = .keepAlways; add(invocation)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"customize","action":{"type":"set_panel_visible","panel":"layers","visible":true}}]"#
        app.launch()
        await ready()
        await waitUntil({ rows.count == 2 })
        await command("open_document")
        enterTestFolder()
        let project = fileProviderFile("RoundTrip.capy", in: app).images.firstMatch
        XCTAssertTrue(project.waitForExistence(timeout: 10)); workspaceActivate(project)
        submitFileProviderSelection(in: app)
        await waitUntil({ rows.count == 3 })
        expectTitle("RoundTrip")
        XCTAssertEqual(rows.count, 3, "The saved layer structure must survive a new process and native Open")
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        capture("project-before-open-pixel-compare", pixels: editorPixels(in: app))
        await waitUntil({ self.editorPixels(in: app) == painted }, timeout: 15)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        capture("project-reopened")
        await restart("RoundTrip", count: 3, pixels: painted)
        let beforeColdOpenSave = try await bytes("RoundTrip.capy")
        XCTAssertEqual(beforeColdOpenSave, originallySaved, "Open and recovery must not rewrite the external source")
        workspaceActivate(app.buttons["layer-New layer"])
        await waitUntil({ rows.count == 4 })
        await command("save_document")
        let savedAfterOpen = try await changedBytes("RoundTrip.capy", from: beforeColdOpenSave)
        expectTitle("RoundTrip")
        XCTAssertEqual(editorPixels(in: app), painted)
        capture("project-cold-open-grant-saved")
        await openProject("RoundTrip", count: 4, pixels: painted)
        await command("save_document_as")
        let secondSave = documentExportControls(in: app)
        enterTestFolder()
        replaceEditorText(secondSave.filename, with: "RoundTripCopy", in: app)
        XCTAssertEqual(secondSave.filename.value as? String, "RoundTripCopy")
        workspaceActivate(secondSave.commit)
        XCTAssertTrue(secondSave.commit.waitForNonExistence(timeout: 20))
        expectTitle("RoundTripCopy")
        let savedCopy = try await bytes("RoundTripCopy.capy")
        let originalAfterCopy = try await bytes("RoundTrip.capy")
        XCTAssertEqual(originalAfterCopy, savedAfterOpen)
        await restart("RoundTripCopy", count: 4, pixels: painted)
        let beforeColdSaveAsSave = try await bytes("RoundTripCopy.capy")
        XCTAssertEqual(beforeColdSaveAsSave, savedCopy, "Recovery must not rewrite the new external destination")
        workspaceActivate(app.buttons["layer-New layer"])
        await waitUntil({ rows.count == 5 })
        await command("save_document")
        let savedAfterSaveAs = try await changedBytes("RoundTripCopy.capy", from: beforeColdSaveAsSave)
        XCTAssertNotEqual(savedAfterSaveAs, savedCopy)
        let originalAfterColdCopySave = try await bytes("RoundTrip.capy")
        XCTAssertEqual(originalAfterColdCopySave, savedAfterOpen, "Saving the new destination must preserve the original file exactly")
        expectTitle("RoundTripCopy")
        XCTAssertEqual(rows.count, 5)
        XCTAssertEqual(editorPixels(in: app), painted)
        capture("project-cold-save-as-grant-saved")
        await openProject("RoundTripCopy", count: 5, pixels: painted)
        await command("export_document")
        chooseExportDestination(in: app)
        let export = documentExportControls(in: app)
        enterTestFolder()
        replaceEditorText(export.filename, with: "Render", in: app)
        workspaceActivate(export.commit)
        XCTAssertTrue(export.commit.waitForNonExistence(timeout: 20))
        XCTAssertEqual(app.state, .runningForeground)
        expectTitle("RoundTripCopy")
        await command("open_document")
        enterTestFolder()
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label == %@ OR label == %@", "Render", "Render.png")).firstMatch.waitForExistence(timeout: 15))
        capture("delivered-project-and-png")
        workspaceActivate(app.buttons["Cancel"].firstMatch)
        expectTitle("RoundTripCopy")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        let delivered = try await Task.detached(priority: .utility) { try FileManager.default.contentsOfDirectory(atPath: root.path).sorted() }.value
        XCTAssertEqual(delivered, ["Render.png", "RoundTrip.capy", "RoundTripCopy.capy"])
        let finalOriginal = try await bytes("RoundTrip.capy"), finalCopy = try await bytes("RoundTripCopy.capy")
        XCTAssertEqual(finalOriginal, savedAfterOpen)
        XCTAssertEqual(finalCopy, savedAfterSaveAs)
    }

    @MainActor func testNativeFilesProjectRoundTripCleanup() async throws {
        continueAfterFailure = false
        let base = try nativeFilesTestFolder()
        let folders = ["light", "dark"].map { base + " " + $0 }
        let provider = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_PACKAGE_PROVIDER"])
        guard provider == "Capy Test Files " + String(base.dropFirst("Capy Files ".count)) else { XCTFail("Private runner provider mismatch"); return }
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        for folder in folders {
            let root = documents.appendingPathComponent(folder, isDirectory: true)
            let owned = try await Task.detached(priority: .utility) {
                let values = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                guard values.isDirectory == true, values.isSymbolicLink != true else { throw CocoaError(.fileReadCorruptFile) }
                return try FileManager.default.contentsOfDirectory(atPath: root.path).sorted()
            }.value
            XCTAssertEqual(owned, ["Render.png", "RoundTrip.capy", "RoundTripCopy.capy"], "Only the generated projects and PNG may be present: " + folder)
        }
        let files = XCUIApplication(bundleIdentifier: "com.apple.DocumentsApp")
        files.activate()
        for folder in folders {
            workspaceActivate(files.cells["DOC.sidebar.item.On My iPad"])
            let container = fileProviderRoot(provider, in: files)
            XCTAssertTrue(container.waitForExistence(timeout: 10)); workspaceActivate(container)
            let cell = files.cells[folder + ", Folder"]
            XCTAssertTrue(cell.waitForExistence(timeout: 10))
            guard cell.staticTexts.matching(NSPredicate(format: "label == %@ OR label ENDSWITH %@", "3 items", " - 3 items"))
                .firstMatch.waitForExistence(timeout: 5) else {
                XCTFail("Only the generated projects and PNG may be present")
                return
            }
            cell.press(forDuration: 0.8)
            workspaceActivate(files.buttons["Delete"].firstMatch)
            XCTAssertTrue(cell.waitForNonExistence(timeout: 10))
        }
    }
}
