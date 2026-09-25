import XCTest

extension XCTestCase {
    @MainActor func checkPalettes(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        func any(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func expect(_ format: String, _ element: XCUIElement, _ args: CVarArg...) {
            expectation(for: NSPredicate(format: format, argumentArray: args), evaluatedWith: element)
            waitForExpectations(timeout: 10)
        }
        func swatches() -> [XCUIElement] {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "palette-swatch-")).allElementsBoundByIndex
                .sorted { ($0.frame.minY, $0.frame.minX) < ($1.frame.minY, $1.frame.minX) }
        }
        func order() -> [String] { swatches().map(\.identifier) }
        func text(_ element: XCUIElement) -> String {
            (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label
        }

        let colorTab = app.buttons["panel-tab-color"], palettesTab = app.buttons["panel-tab-palettes"]
        XCTAssertTrue(palettesTab.waitForExistence(timeout: 10), "Paint places Palettes after Color")
        XCTAssertEqual(palettesTab.frame.minY, colorTab.frame.minY, accuracy: 1)
        XCTAssertGreaterThan(palettesTab.frame.minX, colorTab.frame.minX)
        let group = colorTab.frame
        workspaceActivate(palettesTab)
        let panel = any("palette-panel")
        XCTAssertTrue(panel.waitForExistence(timeout: 10))
        XCTAssertGreaterThanOrEqual(panel.frame.width, 264, "Palettes keep six 40-point columns")
        XCTAssertEqual(colorTab.frame.minY, group.minY, accuracy: 1, "Switching to Palettes keeps the fitted group")
        let first = swatches()
        XCTAssertGreaterThan(first.count, 6)
        XCTAssertEqual(Set(first.prefix(6).map { Int($0.frame.minY.rounded()) }).count, 1, "The first row holds six swatches")
        XCTAssertEqual(first[0].frame.height, 40, accuracy: 1)
        XCTAssertTrue(any("palette-history").exists)
        XCTAssertTrue(any("palette-empty-0").exists, "Recent colors wait for artwork")
        attachEditor(in: app, name: "palettes-paint")

        let detail = any("palette-color-detail")
        let before = text(detail)
        workspaceActivate(first[2])
        expect("selected == YES", first[2])
        expectation(for: NSPredicate { _, _ in text(detail) != before }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(first[2].label.contains(text(detail).components(separatedBy: " · ").first!), "The swatch becomes the paint color")
        XCTAssertEqual(app.buttons["palette-color-name"].label, first[2].label.components(separatedBy: " · ").first)

        let count = swatches().count
        workspaceActivate(app.buttons["palette-add-color"])
        expectation(for: NSPredicate { _, _ in swatches().count == count + 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let added = swatches().last!
        expect("selected == YES", added)

        workspaceActivate(app.buttons["palette-color-name"])
        let editor = app.textFields["palette-name-editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 5))
        #if os(macOS)
        editor.typeKey("a", modifierFlags: .command)
        #endif
        editor.typeText(first[0].label.components(separatedBy: " · ").first! + "\n")
        XCTAssertTrue(any("palette-message").waitForExistence(timeout: 5), "Duplicate names keep the editor open with an error")
        XCTAssertTrue(editor.exists)
        #if os(macOS)
        editor.typeKey("a", modifierFlags: .command)
        #endif
        editor.typeText("Harbor Test\n")
        expect("label == %@", app.buttons["palette-color-name"], "Harbor Test")
        XCTAssertFalse(any("palette-message").exists)

        let selector = app.buttons["palette-chooser"]
        let original = selector.value as? String ?? ""
        workspaceActivate(selector)
        XCTAssertTrue(any("palette-browser").waitForExistence(timeout: 5))
        let search = app.textFields["palette-search"]
        workspaceActivate(search)
        search.typeText("zzzz")
        XCTAssertTrue(any("palette-empty-search").waitForExistence(timeout: 5))
        #if os(macOS)
        search.typeKey("a", modifierFlags: .command)
        #endif
        search.typeText(XCUIKeyboardKey.delete.rawValue)
        let rows = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "palette-choice-"))
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 5))
        XCTAssertGreaterThanOrEqual(rows.count, 10, "Apple installs the starter palettes")
        let other = rows.allElementsBoundByIndex.first { $0.label != original }!
        let otherName = other.label
        workspaceActivate(other)
        XCTAssertTrue(any("palette-browser").waitForNonExistence(timeout: 5), "Choosing a palette closes the chooser")
        expect("value == %@", selector, otherName)

        workspaceActivate(selector)
        workspaceActivate(app.buttons["palette-library-add"])
        let create = app.buttons["palette-command-new_palette"]
        XCTAssertTrue(create.waitForExistence(timeout: 5))
        workspaceActivate(create)
        let name = app.textFields["palette-library-name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.typeText(otherName)
        let save = app.buttons["palette-name-save"]
        expect("enabled == NO", save)
        #if os(macOS)
        name.typeKey("a", modifierFlags: .command)
        #else
        name.doubleTap()
        #endif
        name.typeText("Apple Test Palette")
        expect("enabled == YES", save)
        workspaceActivate(save)
        expect("value == %@", selector, "Apple Test Palette")
        XCTAssertEqual(swatches().count, 0)
        workspaceActivate(app.buttons["palette-add-color"])
        workspaceActivate(app.buttons["palette-add-color"])
        workspaceActivate(app.buttons["palette-add-color"])
        expectation(for: NSPredicate { _, _ in swatches().count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)

        workspaceActivate(selector)
        workspaceActivate(app.buttons["palette-choice-" + String((rows.allElementsBoundByIndex.first { $0.label == otherName }!.identifier)
            .dropFirst("palette-choice-".count))])
        expect("value == %@", selector, otherName)
        let initial = order()
        let from = swatches()[0], to = swatches()[2]
        let start = from.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = to.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        #else
        start.press(forDuration: 0.05, thenDragTo: end)
        #endif
        expectation(for: NSPredicate { _, _ in order() != initial }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertEqual(order()[2], initial[0], "The lifted swatch lands in the third cell")
        XCTAssertEqual(Set(order()), Set(initial))
        XCTAssertFalse(any("palette-drag-preview").exists)
        attachEditor(in: app, name: "palettes-reordered")
        #if os(macOS)
        app.typeKey("z", modifierFlags: .command)
        expectation(for: NSPredicate { _, _ in order() == initial }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        #endif
        let target = swatches()[1], removed = target.identifier
        #if os(macOS)
        target.rightClick()
        #else
        target.press(forDuration: 0.8)
        #endif
        let menu = any("palette-context-menu")
        XCTAssertTrue(menu.waitForExistence(timeout: 5), "Secondary click or a touch hold opens the swatch menu")
        XCTAssertTrue(app.buttons["palette-command-rename_color"].exists)
        workspaceActivate(app.buttons["palette-command-library-remove"])
        expectation(for: NSPredicate { _, _ in !order().contains(removed) }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
