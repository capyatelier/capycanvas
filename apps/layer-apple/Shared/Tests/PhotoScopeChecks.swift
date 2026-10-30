import XCTest

extension XCTestCase {
    @MainActor func checkPhotoScopes(in app: XCUIApplication, theme: String) {
        func launch(_ effect: String) {
            useFreshStorage(app)
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:photographer"}},{"type":"effect","action":{"op":"insert","effect":"\#(effect)"}}]"#
            app.launch()
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
            waitForExpectations(timeout: 30)
        }
        func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func press(_ frame: CGRect) {
            let viewport = workspaceViewport(in: app)
            viewport.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.midX - viewport.frame.minX,
                dy: frame.midY - viewport.frame.minY)).clickOrTap()
        }
        launch("levels")
        XCTAssertTrue(element("scope-histogram").waitForExistence(timeout: 30), "Photo shows the Histogram above Properties")
        let status = app.staticTexts["scope-histogram-status"]
        expectation(for: NSPredicate { _, _ in !status.label.isEmpty || !((status.value as? String) ?? "").isEmpty }, evaluatedWith: status)
        waitForExpectations(timeout: 30)
        XCTAssertTrue(element("scope-histogram-chart").exists)
        XCTAssertTrue(app.buttons["scope-histogram-channel"].exists && app.buttons["scope-histogram-source"].exists)
        XCTAssertTrue(element("scope-tonal_histogram").waitForExistence(timeout: 10), "Levels shows its tonal histogram")
        XCTAssertTrue(app.buttons["property-action-auto_levels"].exists, "Levels offers Auto")
        let calibration = app.buttons["property-action-group-calibration"]
        XCTAssertTrue(calibration.exists, "Levels offers the calibration pickers")
        let frame = calibration.frame
        workspaceActivate(calibration)
        XCTAssertTrue(app.buttons["property-calibrate-gray"].waitForExistence(timeout: 5))
        attachEditor(in: app, name: "photo-scopes-levels-\(theme)")
        press(frame)
        XCTAssertTrue(app.buttons["property-calibrate-gray"].waitForNonExistence(timeout: 5))
        let highlights = app.buttons["scope-histogram-highlights"]
        XCTAssertFalse(highlights.isSelected)
        workspaceActivate(highlights)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: highlights)
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["panel-tab-waveform"])
        XCTAssertTrue(element("scope-waveform-chart").waitForExistence(timeout: 10), "The Waveform shares the scope column")
        attachEditor(in: app, name: "photo-scopes-waveform-\(theme)")
        let viewport = workspaceViewport(in: app)
        let group = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-group-"))
            .containing(.button, identifier: "panel-tab-histogram").firstMatch
        let grip = group.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "group-options-")).firstMatch
        XCTAssertTrue(grip.waitForExistence(timeout: 5))
        let history = ["Undo", "Redo"].map { label in
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", label)).firstMatch
        }
        XCTAssertTrue(history.allSatisfy(\.exists))
        let sample = CGPoint(x: 0.25, y: 0.6)
        let samplePoint = CGPoint(x: viewport.frame.minX + viewport.frame.width * sample.x,
            y: viewport.frame.minY + viewport.frame.height * sample.y)
        XCTAssertFalse(group.frame.insetBy(dx: -8, dy: -8).contains(samplePoint))
        let pixels = editorPixels(in: app, at: sample)
        let enabled = history.map(\.isEnabled)
        func drag(_ start: XCUICoordinate, to target: XCUICoordinate) {
            #if os(macOS)
            start.click(forDuration: 0.1, thenDragTo: target)
            #else
            start.press(forDuration: 0.1, thenDragTo: target)
            #endif
        }
        let bounds = group.frame, handle = grip.frame
        drag(grip.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)),
            to: viewport.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (viewport.frame.width - bounds.width) / 2 + handle.midX - bounds.minX,
                dy: (viewport.frame.height - 550) / 2 + handle.midY - bounds.minY)))
        let groupID = String(group.identifier.dropFirst("workspace-group-".count))
        let bottom = element("workspace-resize-\(groupID)-bottom")
        XCTAssertTrue(bottom.waitForExistence(timeout: 10), "The scope group must float with its real bottom handle")
        let parts = ["source", "channel", "chart", "log", "status", "shadows", "highlights"]
        func frames(_ kind: String) -> [String: CGRect]? {
            var values: [String: CGRect] = [:]
            for part in parts {
                let control = group.descendants(matching: .any)["scope-\(kind)-\(part)"].firstMatch
                guard control.exists else { return nil }
                let frame = control.frame
                guard [frame.minX, frame.minY, frame.width, frame.height].allSatisfy(\.isFinite),
                    frame.width > 0, frame.height > 0 else { return nil }
                values[part] = frame
            }
            return values
        }
        func resize(_ kind: String, to height: CGFloat) {
            XCTAssertTrue(viewport.frame.contains(group.frame))
            XCTAssertTrue(viewport.frame.contains(bottom.frame) && bottom.frame.width > 0 && bottom.frame.height > 0)
            let start = bottom.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            drag(start, to: start.withOffset(CGVector(dx: 0, dy: height - group.frame.height)))
            expectation(for: NSPredicate { _, _ in
                abs(group.frame.height - height) < 0.02 && frames(kind) != nil
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
            attachEditor(in: app, name: "photo-scopes-\(kind)-\(Int(height))-\(theme)")
        }
        func ordered(_ values: [String: CGRect]) -> Bool {
            values["source"]!.maxY <= values["chart"]!.minY
                && values["channel"]!.maxY <= values["chart"]!.minY
                && values["chart"]!.maxY <= values["log"]!.minY
                && ["status", "shadows", "highlights"].allSatisfy { values["log"]!.maxY <= values[$0]!.minY }
        }
        for kind in ["histogram", "waveform"] {
            workspaceActivate(app.buttons["panel-tab-\(kind)"])
            let status = app.staticTexts["scope-\(kind)-status"]
            expectation(for: NSPredicate { _, _ in !status.label.isEmpty || !((status.value as? String) ?? "").isEmpty }, evaluatedWith: status)
            waitForExpectations(timeout: 30)
            resize(kind, to: 450)
            let logarithmic = group.descendants(matching: .any)["scope-\(kind)-log"].firstMatch
            let selected = String(describing: logarithmic.value ?? "")
            workspaceActivate(logarithmic)
            expectation(for: NSPredicate { _, _ in String(describing: logarithmic.value ?? "") == (selected == "1" ? "0" : "1") }, evaluatedWith: logarithmic)
            waitForExpectations(timeout: 10)
            workspaceActivate(logarithmic)
            expectation(for: NSPredicate { _, _ in String(describing: logarithmic.value ?? "") == selected }, evaluatedWith: logarithmic)
            waitForExpectations(timeout: 10)
            guard let first = frames(kind) else { XCTFail("The scope must expose seven positive native frames"); return }
            let firstBounds = group.frame
            XCTAssertFalse(firstBounds.insetBy(dx: -8, dy: -8).contains(samplePoint))
            XCTAssertTrue(ordered(first))
            XCTAssertEqual(editorPixels(in: app, at: sample), pixels)
            XCTAssertEqual(history.map(\.isEnabled), enabled)
            resize(kind, to: 550)
            guard let grown = frames(kind) else { XCTFail("The grown scope must retain seven positive native frames"); return }
            let grownBounds = group.frame
            XCTAssertFalse(grownBounds.insetBy(dx: -8, dy: -8).contains(samplePoint))
            XCTAssertGreaterThan(grown["chart"]!.height, first["chart"]!.height)
            XCTAssertEqual(grown["chart"]!.height - first["chart"]!.height,
                grownBounds.height - firstBounds.height, accuracy: 0.02)
            XCTAssertTrue(ordered(grown))
            for part in parts where part != "chart" { XCTAssertEqual(grown[part]!.height, first[part]!.height, accuracy: 0.02) }
            XCTAssertEqual(grown["log"]!.width, first["log"]!.width, accuracy: 0.02)
            XCTAssertEqual(editorPixels(in: app, at: sample), pixels)
            XCTAssertEqual(history.map(\.isEnabled), enabled)
            resize(kind, to: 450)
            guard let restored = frames(kind) else { XCTFail("The restored scope must retain seven positive native frames"); return }
            XCTAssertFalse(group.frame.insetBy(dx: -8, dy: -8).contains(samplePoint))
            XCTAssertTrue(ordered(restored))
            XCTAssertEqual(restored["chart"]!.height, first["chart"]!.height, accuracy: 0.02)
            XCTAssertEqual(editorPixels(in: app, at: sample), pixels)
            XCTAssertEqual(history.map(\.isEnabled), enabled)
            let geometry = XCTAttachment(string: "kind=\(kind) theme=\(theme) first_group=\(firstBounds) grown_group=\(grownBounds) restored_group=\(group.frame) first=\(first) grown=\(grown) restored=\(restored)")
            geometry.name = "photo-scopes-allocation-\(kind)-\(theme)"; geometry.lifetime = .keepAlways; add(geometry)
        }
        app.terminate()

        launch("curves")
        XCTAssertTrue(app.staticTexts["scope-tonal_histogram-status"].waitForExistence(timeout: 30), "Curves shows its histogram status")
        XCTAssertTrue(app.buttons["scope-tonal_histogram-highlights"].exists, "Curves offers the clipping overlays")
        XCTAssertTrue(app.buttons["property-action-target_curve"].exists, "Curves offers targeted adjustment")
        attachEditor(in: app, name: "photo-scopes-curves-\(theme)")
        app.terminate()

        launch("color_lookup")
        let choice = app.buttons["property-resource-choice"]
        XCTAssertTrue(choice.waitForExistence(timeout: 30), "Color lookup offers its tables")
        XCTAssertTrue(app.buttons["import-lookup"].exists, "Color lookup offers .cube import")
        let original = choice.value as? String
        workspaceActivate(choice); workspaceActivate(app.buttons["property-resource-choice-option-1"])
        expectation(for: NSPredicate { _, _ in choice.value as? String != original }, evaluatedWith: choice)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "photo-scopes-lookup-\(theme)")
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in choice.value as? String == original }, evaluatedWith: choice)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
