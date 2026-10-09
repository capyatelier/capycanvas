import XCTest
#if os(macOS)
import AppKit
#endif

extension XCTestCase {
    @MainActor func checkPixelClipboard(in app: XCUIApplication, theme: String) throws {
        #if os(macOS)
        let pasteboard = try NativePhotoPasteboard()
        let marker = NSPasteboardItem()
        marker.setString(UUID().uuidString, forType: .string)
        try pasteboard.replace([marker])
        defer { pasteboard.restore() }
        #else
        let monitor = addUIInterruptionMonitor(withDescription: "Clipboard source") { alert in
            guard alert.buttons["Allow Paste"].exists else { return false }
            let source = XCTAttachment(string: alert.debugDescription)
            source.name = "clipboard-paste-source-" + theme; source.lifetime = .keepAlways; self.add(source)
            let capture = XCTAttachment(screenshot: alert.screenshot())
            capture.name = "clipboard-paste-alert-" + theme; capture.lifetime = .keepAlways; self.add(capture)
            return false
        }
        defer { removeUIInterruptionMonitor(monitor) }
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let before = rows.count
        let point = editorPaper(in: app).point(0.5, 0.5)
        func count(_ expected: Int) {
            expectation(for: NSPredicate { _, _ in rows.count == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
        }
        func copy(_ id: String, _ label: String) {
            #if os(macOS)
            let previous = pasteboard.board.changeCount
            switch id {
            case "copy": app.typeKey("c", modifierFlags: .command)
            case "cut": app.typeKey("x", modifierFlags: .command)
            default: editorMenu(in: app, menu: "Edit", id: id, label: label)
            }
            #else
            editorMenu(in: app, menu: "Edit", id: id, label: label)
            #endif
            #if os(macOS)
            expectation(for: NSPredicate { _, _ in
                pasteboard.board.changeCount != previous && pasteboard.board.data(forType: .png).map { NSImage(data: $0) != nil } == true
            }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
            pasteboard.changeCount = pasteboard.board.changeCount
            #endif
        }
        func paste() {
            #if os(macOS)
            app.typeKey("v", modifierFlags: .command)
            #else
            editorMenu(in: app, menu: "Edit", id: "paste_image", label: "Paste")
            #endif
        }
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app, at: point); return Int(sample[2]) > Int(sample[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 20)
        let painted = editorPixels(in: app, at: point)
        copy("copy_merged", "Copy Merged")
        paste()
        count(before + 1); expectPixels(painted, in: app, at: point)
        XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", "Merged copy")).firstMatch.exists,
            "Pasting this window's copy adds its full-depth clip, not an imported image")
        attachEditor(in: app, name: "pixel-clipboard-" + theme)
        editorHistory("Undo", in: app); count(before); expectPixels(painted, in: app, at: point)
        editorHistory("Redo", in: app); count(before + 1); expectPixels(painted, in: app, at: point)
        editorHistory("Undo", in: app); count(before)
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        copy("copy", "Copy")
        paste()
        count(before + 1); expectPixels(painted, in: app, at: point)
        XCTAssertTrue(app.staticTexts["Paint layer"].exists, "Layer copies remain editable paint layers")
        copy("cut", "Cut"); count(before); expectPixels(painted, in: app, at: point)
        editorHistory("Undo", in: app); count(before + 1); expectPixels(painted, in: app, at: point)
        editorHistory("Redo", in: app); count(before); expectPixels(painted, in: app, at: point)
        editorHistory("Undo", in: app); count(before + 1)
        #if os(macOS)
        app.typeKey("n", modifierFlags: [.command, .option])
        #else
        editorMenu(in: app, menu: "Edit", id: "paste_as_new_image", label: "Paste as New Image")
        #endif
        let second = app.buttons["drawing-tab-2"].firstMatch
        if !second.waitForExistence(timeout: 5) { workspaceActivate(app.buttons["document-title"]) }
        XCTAssertTrue(second.waitForExistence(timeout: 20)); XCTAssertTrue(second.isSelected)
        XCTAssertTrue((second.value as? String ?? "").contains("2048 × 1536"))
        if app.buttons["Done"].firstMatch.exists { workspaceActivate(app.buttons["Done"].firstMatch) }
        count(before)
        expectPixels(painted, in: app, at: point)
        XCTAssertTrue(app.staticTexts["Paint layer"].exists)
        attachEditor(in: app, name: "layer-clipboard-new-image-" + theme)
        let first = app.buttons["drawing-tab-1"].firstMatch
        if !first.exists { workspaceActivate(app.buttons["document-title"]) }
        workspaceActivate(first)
        count(before + 1); expectPixels(painted, in: app, at: point)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
