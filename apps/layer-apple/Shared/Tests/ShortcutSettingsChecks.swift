import XCTest

extension XCTestCase {
    @MainActor func checkShortcutSettingsPage(in app: XCUIApplication) {
        app.launch(); capturePaintEditor(in: app)
        func any(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func expect(_ element: XCUIElement, _ format: String, _ message: String = "") {
            expectation(for: NSPredicate(format: format), evaluatedWith: element)
            waitForExpectations(timeout: 10)
        }
        func back() { workspaceActivate(app.buttons["settings-back"]) }
        workspaceActivate(app.buttons["settings-button"])
        workspaceActivate(app.staticTexts["settings-page-shortcuts"])
        let search = app.textFields["shortcut-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        for id in ["keymap-preset", "keymap-menu", "shortcut-context", "shortcut-show"] {
            XCTAssertTrue(any(id).exists, "The shortcut page shows \(id)")
        }
        let modifiers = app.buttons["shortcut-category-Modifier keys"], tools = app.buttons["shortcut-category-Tools"]
        XCTAssertTrue(modifiers.waitForExistence(timeout: 5)); XCTAssertTrue(tools.exists)
        attachEditor(in: app, name: "shortcuts-categories")

        workspaceActivate(modifiers)
        let space = app.buttons["modifier-Space"]
        XCTAssertTrue(space.waitForExistence(timeout: 10), "Modifier keys list the default Space hold")
        XCTAssertTrue(app.buttons["add-modifier-key"].exists)
        XCTAssertFalse(tools.exists, "A category replaces the category list")
        workspaceActivate(space)
        let action = app.buttons["modifier-action-all"]
        XCTAssertTrue(action.waitForExistence(timeout: 10), "Space does the same with every tool")
        XCTAssertTrue(any("modifier-same").exists)
        workspaceActivate(action)
        let pen = app.buttons["action-command.Pen"]
        XCTAssertTrue(pen.waitForExistence(timeout: 10), "The action picker lists tools")
        attachEditor(in: app, name: "shortcuts-action-picker")
        workspaceActivate(pen)
        XCTAssertTrue(pen.waitForNonExistence(timeout: 10))
        let reset = app.buttons["modifier-reset"]
        XCTAssertTrue(reset.waitForExistence(timeout: 10), "A changed modifier key offers Reset")
        workspaceActivate(reset)
        XCTAssertTrue(reset.waitForNonExistence(timeout: 10))
        back()
        XCTAssertTrue(space.waitForExistence(timeout: 10))

        workspaceActivate(app.buttons["add-modifier-key"])
        let captured = app.staticTexts["shortcut-captured"]
        XCTAssertTrue(captured.waitForExistence(timeout: 10))
        app.typeKey("q", modifierFlags: [])
        let confirm = app.buttons["shortcut-confirm"]
        expect(confirm, "enabled == YES")
        workspaceActivate(confirm)
        let remove = app.buttons["modifier-remove"]
        XCTAssertTrue(remove.waitForExistence(timeout: 10), "Adding a modifier key opens it")
        attachEditor(in: app, name: "shortcuts-new-modifier")
        workspaceActivate(remove)
        XCTAssertTrue(space.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["modifier-Q"].exists, "Removing the modifier key deletes its row")
        back()
        XCTAssertTrue(tools.waitForExistence(timeout: 10))

        workspaceActivate(search); search.typeText("Zen")
        let zen = app.buttons["shortcut-command.ZenMode"]
        XCTAssertTrue(zen.waitForExistence(timeout: 10))
        XCTAssertFalse(tools.exists, "Searching lists matching actions instead of categories")
        search.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 3))
        XCTAssertTrue(tools.waitForExistence(timeout: 10))
        #if os(macOS)
        workspaceActivate(search)
        app.typeKey("t", modifierFlags: .command)
        XCTAssertTrue(app.buttons["shortcut-command.ScaleRotate"].waitForExistence(timeout: 10), "A pressed chord finds its action")
        attachEditor(in: app, name: "shortcuts-chord-search")
        #endif

        func menuItem(_ label: String) -> XCUIElement {
            #if os(macOS)
            return any("keymap-menu").menuItems[label]
            #else
            return app.buttons[label].firstMatch
            #endif
        }
        workspaceActivate(any("keymap-menu"))
        workspaceActivate(menuItem("Differences…"))
        let details = app.buttons["keymap-details-done"]
        XCTAssertTrue(details.waitForExistence(timeout: 10), "Differences opens the keymap details")
        attachEditor(in: app, name: "shortcuts-keymap-details")
        workspaceActivate(details)
        XCTAssertTrue(details.waitForNonExistence(timeout: 10))
        #if os(macOS)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Keymap " + UUID().uuidString)
        XCTAssertNoThrow(try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false))
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("capycanvas.capykeys")
        workspaceActivate(any("keymap-menu")); workspaceActivate(menuItem("Export…"))
        let export = app.windows.buttons["Export"].firstMatch
        XCTAssertTrue(export.waitForExistence(timeout: 15), "Export opens the save panel")
        attachEditor(in: app, name: "shortcuts-keymap-export")
        app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(root.path + "\n")
        export.click()
        expectation(for: NSPredicate { _, _ in FileManager.default.fileExists(atPath: file.path) }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        workspaceActivate(any("keymap-menu")); workspaceActivate(menuItem("Import…"))
        let open = app.windows.buttons["Open"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 15), "Import opens the open panel")
        app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(file.path + "\n")
        expect(open, "enabled == YES")
        open.click()
        let confirmImport = app.buttons["confirm-keymap-import"]
        XCTAssertTrue(confirmImport.waitForExistence(timeout: 15), "An exported keymap imports with a preview")
        attachEditor(in: app, name: "shortcuts-keymap-import")
        workspaceActivate(confirmImport)
        XCTAssertTrue(confirmImport.waitForNonExistence(timeout: 10))
        #else
        for label in ["Export…", "Import…"] {
            workspaceActivate(any("keymap-menu"))
            workspaceActivate(menuItem(label))
            let cancel = app.buttons["Cancel"].firstMatch
            XCTAssertTrue(cancel.waitForExistence(timeout: 15), "\(label) opens the system file browser")
            attachEditor(in: app, name: "shortcuts-\(label)")
            cancel.clickOrTap()
            XCTAssertTrue(search.waitForExistence(timeout: 10))
        }
        #endif
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
