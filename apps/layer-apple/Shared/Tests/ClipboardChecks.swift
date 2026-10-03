import XCTest
#if os(macOS)
import AppKit
#endif

extension XCTestCase {
    @MainActor func checkPixelClipboard(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch(); capturePaintEditor(in: app)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let before = rows.count
        #if os(macOS)
        NSPasteboard.general.clearContents()
        app.typeKey("a", modifierFlags: .command)
        app.typeKey("c", modifierFlags: [.command, .shift])
        expectation(for: NSPredicate { _, _ in NSPasteboard.general.data(forType: .png).map { NSImage(data: $0) != nil } ?? false },
            evaluatedWith: app)
        waitForExpectations(timeout: 30)
        app.typeKey("v", modifierFlags: .command)
        #else
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "copy_merged", label: "Copy Merged")
        editorMenu(in: app, menu: "Edit", id: "paste_image", label: "Paste")
        #endif
        expectation(for: NSPredicate { _, _ in rows.count == before + 1 }, evaluatedWith: app); waitForExpectations(timeout: 30)
        XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", "Merged copy")).firstMatch.exists,
            "Pasting this window's copy adds its full-depth clip, not an imported image")
        attachEditor(in: app, name: "pixel-clipboard")
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in rows.count == before }, evaluatedWith: app); waitForExpectations(timeout: 20)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
