import XCTest

extension XCTestCase {
    @MainActor func checkTouchGestureTaps(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let paper = editorPixels(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let painted = editorPixels(in: app)
        let trigger = app.buttons["trigger-touch.tap.2"]
        func openInputSettings() {
            workspaceActivate(app.buttons["settings-button"])
            workspaceActivate(app.staticTexts["settings-page-input"])
            #if os(iOS)
            let list = app.collectionViews.containing(.button, identifier: "preference-cursor").firstMatch
            XCTAssertTrue(list.waitForExistence(timeout: 10))
            for _ in 0..<6 where !trigger.exists { list.swipeUp() }
            #endif
        }
        func closeSettings() {
            workspaceActivate(app.buttons["settings-done"])
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 10))
        }
        #if os(iOS)
        canvas.twoFingerTap()
        expectPixels(paper, in: app)
        canvas.tap(withNumberOfTaps: 1, numberOfTouches: 3)
        expectPixels(painted, in: app)
        attachEditor(in: app, name: "touch-taps")

        openInputSettings()
        XCTAssertTrue(trigger.waitForExistence(timeout: 10), "Pen & Input lists finger taps")
        workspaceActivate(trigger)
        let nothing = app.buttons["action-nothing"]
        XCTAssertTrue(nothing.waitForExistence(timeout: 10))
        attachEditor(in: app, name: "touch-tap-picker")
        workspaceActivate(nothing)
        XCTAssertTrue(nothing.waitForNonExistence(timeout: 10))
        attachEditor(in: app, name: "touch-tap-nothing")
        closeSettings()
        canvas.twoFingerTap()
        Thread.sleep(forTimeInterval: 1)
        expectPixels(painted, in: app)

        openInputSettings()
        workspaceActivate(trigger)
        let reset = app.buttons["action-picker-reset"]
        XCTAssertTrue(reset.waitForExistence(timeout: 10), "A changed tap offers Reset")
        workspaceActivate(reset)
        XCTAssertTrue(reset.waitForNonExistence(timeout: 10))
        closeSettings()
        canvas.twoFingerTap()
        expectPixels(paper, in: app)
        #else
        openInputSettings()
        let lower = app.buttons["trigger-pen.button.primary"]
        let list = app.scrollViews.containing(.any, identifier: "preference-cursor").firstMatch
        for _ in 0..<12 where !lower.isHittable { list.scroll(byDeltaX: 0, deltaY: -200) }
        XCTAssertTrue(lower.waitForExistence(timeout: 10), "Pen & Input lists tablet side buttons on macOS")
        XCTAssertFalse(trigger.exists, "macOS delivers no finger taps")
        workspaceActivate(lower)
        let all = app.buttons["pen-button-action-all"]
        XCTAssertTrue(all.waitForExistence(timeout: 10), "A side button opens its per-tool page")
        attachEditor(in: app, name: "pen-button-page")
        workspaceActivate(all)
        let search = app.textFields["action-picker-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        workspaceActivate(search); search.typeText("Pan")
        let hand = app.buttons["action-command.Hand"]
        XCTAssertTrue(hand.waitForExistence(timeout: 10))
        workspaceActivate(hand)
        XCTAssertTrue(hand.waitForNonExistence(timeout: 10))
        let reset = app.buttons["pen-button-reset"]
        XCTAssertTrue(reset.waitForExistence(timeout: 10), "A bound side button offers Reset")
        workspaceActivate(reset)
        XCTAssertTrue(reset.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["settings-back"])
        XCTAssertTrue(lower.waitForExistence(timeout: 10))
        closeSettings()
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
