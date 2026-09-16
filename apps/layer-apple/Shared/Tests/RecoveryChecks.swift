import XCTest

extension XCTestCase {
    @MainActor func checkArtworkRecoveryAfterRestart(in app: XCUIApplication) {
        app.launchEnvironment.removeValue(forKey: "CAPY_DISABLE_PERSISTENCE")
        app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] = UUID().uuidString
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"add_layer"},{"type":"set_layer_opacity","opacity":0.42},{"type":"set_color","rgba":[0.1,0.3,0.9,1]}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let ready = app.staticTexts["recovery-status"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Recovery ready", "Recovery ready"), evaluatedWith: ready)
        waitForExpectations(timeout: 30)
        // Fill through the enabled native menus after startup, not through
        // initial actions that can run before canvas commands are available.
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 20
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let painted = editorPixels(in: app)
        let scene = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-")).firstMatch
        let sceneID = scene.identifier
        XCTAssertFalse(sceneID.isEmpty)
        // Exercise the OS lifecycle before waiting for the latest recovery
        // copy. Returning must preserve this editor and its editable history.
        #if os(macOS)
        app.typeKey("h", modifierFlags: .command)
        #else
        XCUIDevice.shared.press(.home)
        #endif
        expectation(for: NSPredicate { _, _ in
            #if os(macOS)
            app.state == .runningBackground
            #else
            [.runningBackground, .runningBackgroundSuspended].contains(app.state)
            #endif
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        app.activate()
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == painted }, evaluatedWith: app)
        waitForExpectations(timeout: 20)
        XCTAssertEqual(scene.identifier, sceneID, "Returning must preserve the existing editor scene")
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) != painted }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        editorHistory("Redo", in: app)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == painted }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        // Deselect has its own history entry, so do it after checking that
        // the pre-background fill can still be undone and redone.
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Recovery ready", "Recovery ready"), evaluatedWith: ready)
        waitForExpectations(timeout: 30)
        attachEditor(in: app, name: "unsaved-artwork-before-restart")
        app.terminate()
        app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS")
        app.launch()
        let open = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "open-recovery-")).firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 20), "An unclosed drawing must be offered after process restart")
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: open)
        waitForExpectations(timeout: 30)
        attachEditor(in: app, name: "recovered-drawings-picker")
        #if os(macOS)
        open.click()
        let rows = app.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #else
        open.tap()
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #endif
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        XCTAssertTrue(open.waitForNonExistence(timeout: 5))
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == painted }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Recovery ready", "Recovery ready"), evaluatedWith: ready)
        waitForExpectations(timeout: 30)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        attachEditor(in: app, name: "recovered-drawing-editor")
        app.terminate()
    }
}
