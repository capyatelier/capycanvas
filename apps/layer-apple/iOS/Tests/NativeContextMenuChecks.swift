import XCTest

extension EditorLaunchTests {
    @MainActor func testNativeZenScreenEdges() {
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            for edge in ["top", "bottom"] {
                let app = editorTestApplication()
                app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:illustrator"}},{"type":"move_panel","panel":"toolbar","target":{"kind":"edge","edge":"\#(edge)","outer":true},"viewport":[1376,1032]}]"#
                app.launch()
                let toolbar = app.descendants(matching: .any)["toolbar-options-toolbar"].firstMatch
                XCTAssertTrue(toolbar.waitForExistence(timeout: 30))
                waitForEditorInteraction(in: app)
                attachEditor(in: app, name: "zen-edge-initial-\(edge)-\(theme)")
                workspaceActivate(app.descendants(matching: .any)["zen-button"].firstMatch)
                expectation(for: NSPredicate { _, _ in !toolbar.isEnabled }, evaluatedWith: toolbar)
                waitForExpectations(timeout: 10)
                let screen = app.frame
                XCTAssertEqual(screen.minX, 0, accuracy: 1)
                XCTAssertEqual(screen.minY, 0, accuracy: 1)
                let origin = app.coordinate(withNormalizedOffset: .zero)
                let startY = edge == "top" ? CGFloat(1) : screen.height - 1
                let endY = edge == "top" ? CGFloat(161) : screen.height - 161
                origin.withOffset(CGVector(dx: screen.width / 2, dy: startY))
                    .press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: screen.width / 2, dy: endY)))
                attachEditor(in: app, name: "zen-edge-swipe-\(edge)-\(theme)")
                XCTAssertEqual(app.state, .runningForeground)
                let tool = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-toolbar-", "Eraser")).firstMatch
                expectation(for: NSPredicate { _, _ in
                    toolbar.isEnabled && tool.exists && tool.isEnabled && !tool.frame.isEmpty && app.frame.contains(tool.frame)
                }, evaluatedWith: toolbar)
                waitForExpectations(timeout: 10)
                XCTAssertFalse(tool.isSelected)
                let bounds = tool.frame
                app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: bounds.midX - app.frame.minX,
                    dy: bounds.midY - app.frame.minY)).tap()
                expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: tool)
                waitForExpectations(timeout: 10)
                attachEditor(in: app, name: "zen-edge-use-\(edge)-\(theme)")
                XCTAssertFalse(app.staticTexts["Canvas error"].exists)
                app.terminate()
            }
        }
    }

    @MainActor func testDroppedUnmeasuredTabsFit() {
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorTestApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:illustrator"}},{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":false}},{"type":"move_panel","panel":"stats","target":{"kind":"tab","group":7},"viewport":[1376,1032]},{"type":"select_panel_tab","group":7,"panel":"stats"}]"#
            app.launch()
            let group = app.descendants(matching: .any)["workspace-group-7"].firstMatch
            let grip = app.descendants(matching: .any)["group-options-7"].firstMatch
            XCTAssertTrue(grip.waitForExistence(timeout: 30))
            waitForEditorInteraction(in: app)
            XCTAssertTrue(app.buttons["panel-tab-stats"].isSelected)
            XCTAssertTrue(app.descendants(matching: .any)["renderer-stats"].firstMatch.exists)
            XCTAssertFalse(app.buttons["size-0.7"].exists)
            attachEditor(in: app, name: "unmeasured-tabs-initial-\(theme)")
            let initial = group.frame
            let destination = workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.55, dy: 0.5))
            grip.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                .press(forDuration: 0.1, thenDragTo: destination)
            expectation(for: NSPredicate { _, _ in group.exists && group.frame.minX > initial.maxX + 20 }, evaluatedWith: group)
            waitForExpectations(timeout: 10)
            XCTContext.runActivity(named: "\(theme) initial=\(initial) dropped=\(group.frame)") { _ in
                attachEditor(in: app, name: "unmeasured-tabs-drop-\(theme)")
            }
            for panel in ["stats", "sizes", "stats"] {
                workspaceActivate(app.buttons["panel-tab-\(panel)"])
                let witness = panel == "sizes" ? app.buttons["size-2000"] : app.buttons["stroke-recording"]
                XCTAssertTrue(witness.waitForExistence(timeout: 10))
                let content = panel == "sizes"
                    ? group.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "size-")).allElementsBoundByIndex
                    : [app.descendants(matching: .any)["renderer-stats"].firstMatch, witness]
                if panel == "sizes" { XCTAssertEqual(content.count, 40) }
                expectation(for: NSPredicate { _, _ in
                    let bounds = group.frame.insetBy(dx: -1, dy: -1)
                    return content.allSatisfy { $0.exists && !$0.frame.isEmpty && bounds.contains($0.frame) }
                }, evaluatedWith: group)
                waitForExpectations(timeout: 10)
                XCTContext.runActivity(named: "\(theme) \(panel) group=\(group.frame) content=\(content.map(\.frame))") { _ in
                    attachEditor(in: app, name: "unmeasured-tabs-\(panel)-\(theme)")
                }
            }
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            app.terminate()
        }
    }

    @MainActor func testNativeZenContextAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorTestApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"customize","action":{"type":"set_panel_visible","panel":"layers","visible":true}}]"#
            app.launch()
            let zen = app.descendants(matching: .any)["zen-button"].firstMatch.buttons.firstMatch
            XCTAssertTrue(zen.waitForExistence(timeout: 20))
            zen.press(forDuration: 0.8)
            let icon = app.buttons["Change icon…"]
            XCTAssertTrue(icon.waitForExistence(timeout: 5))
            XCTAssertTrue(app.buttons["layer-New layer"].exists, "Opening the native menu must suppress the Zen button tap")
            icon.tap()
            let choices = (0..<4).map { app.buttons["preference-zen_icon-\($0)"] }
            for choice in choices { XCTAssertTrue(choice.waitForExistence(timeout: 10)) }
            XCTAssertEqual(choices.filter { $0.isSelected }.count, 1)
            XCTAssertTrue(choices[0].isSelected)
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            app.terminate()
        }
    }
}

extension XCTestCase {
    @MainActor func checkNativeWorkspaceContextAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"},{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let painter = app.descendants(matching: .any)["workspace-select-builtin:workspace:painter"].firstMatch
        let illustrator = app.descendants(matching: .any)["workspace-select-builtin:workspace:illustrator"].firstMatch
        let photographer = app.descendants(matching: .any)["workspace-select-builtin:workspace:photographer"].firstMatch
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        workspaceActivate(illustrator)
        XCTAssertTrue(illustrator.isSelected)
        painter.press(forDuration: 0.8)
        let pin = app.buttons["Show in top bar"]
        let down = app.buttons["Move Down"]
        XCTAssertTrue(pin.waitForExistence(timeout: 5))
        XCTAssertTrue(down.waitForExistence(timeout: 5))
        XCTAssertLessThan(pin.frame.maxY, down.frame.minY, "Row menus must retain a vertical layout")
        XCTAssertTrue(illustrator.isSelected, "A hold must suppress the row's ordinary selection")
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        capture.name = "native-workspace-context-dark"; capture.lifetime = .keepAlways; add(capture)
        down.tap()
        XCTAssertTrue(down.waitForNonExistence(timeout: 5))
        expectation(for: NSPredicate { _, _ in painter.frame.minY > illustrator.frame.minY && painter.frame.minY < photographer.frame.minY }, evaluatedWith: painter)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(illustrator.isSelected)
        app.terminate(); app.launch()
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        XCTAssertGreaterThan(painter.frame.minY, illustrator.frame.minY, "The native action must persist the shared order")
        XCTAssertLessThan(painter.frame.minY, photographer.frame.minY)
    }
}
