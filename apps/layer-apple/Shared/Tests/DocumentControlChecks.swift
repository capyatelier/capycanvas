import XCTest
import ImageIO
import UniformTypeIdentifiers

#if os(macOS)
extension XCTestCase {
    @MainActor func checkFailedProjectOpenPreservesArtwork(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Failed Open " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let project = root.appendingPathComponent("Survivor.capy")
        let invalid = root.appendingPathComponent("Invalid.capy")
        let invalidBytes = Data("This is not a Capy Canvas drawing".utf8)
        try invalidBytes.write(to: invalid)
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let title = app.staticTexts["document-title"]
        func titleText() -> String { title.value as? String ?? title.label }
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        func command(_ id: String, _ label: String) {
            editorMenu(in: app, menu: "File", id: id, label: label)
        }
        func goTo(_ url: URL) {
            app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(url.path + "\n")
        }
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func chooseOpen(_ url: URL) {
            let open = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(open.waitForExistence(timeout: 15))
            goTo(url); workspaceActivate(open)
            XCTAssertTrue(open.waitForNonExistence(timeout: 15))
        }
        let paper = editorPixels(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app); return Int(sample[2]) > Int(sample[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let painted = editorPixels(in: app)
        command("save_document_as", "Save As…")
        let save = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(save.waitForExistence(timeout: 15)); goTo(root)
        let name = app.textFields["saveAsNameTextField"]
        workspaceActivate(name)
        name.typeKey("a", modifierFlags: .command); name.typeText(project.lastPathComponent)
        workspaceActivate(save)
        XCTAssertTrue(save.waitForNonExistence(timeout: 15))
        expectation(for: NSPredicate { _, _ in titleText().hasPrefix("Survivor.capy · ") }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let saved = try Data(contentsOf: project)
        let savedTitle = titleText(), savedRows = rows.count
        // SwiftUI presents these native Mac alerts as sheets. Scope their
        // buttons to that sheet, excluding the Touch Bar's duplicate actions.
        let dialog = app.sheets.firstMatch

        editorMenu(in: app, menu: "Edit", id: "clear_layer", label: "Clear layer")
        expectPixels(paper)
        command("open_document", "Open…")
        workspaceActivate(dialog.buttons["Discard Changes"])
        chooseOpen(invalid)
        let failure = dialog
        XCTAssertTrue(failure.waitForExistence(timeout: 20), "An invalid project must report its error")
        XCTAssertTrue(failure.staticTexts["Document"].exists)
        XCTAssertGreaterThan(failure.staticTexts.count, 1, "The error must explain why the project could not open")
        workspaceActivate(failure.buttons["OK"])
        XCTAssertTrue(failure.waitForNonExistence(timeout: 10))
        XCTAssertEqual(titleText(), savedTitle); XCTAssertEqual(rows.count, savedRows)
        expectPixels(paper)
        for (action, pixels) in [("Undo", painted), ("Redo", paper)] {
            editorHistory(action, in: app); expectPixels(pixels)
        }
        attachEditor(in: app, name: "failed-open-preserves-unsaved-artwork-and-history")

        // A failed replacement must not mark the original drawing clean, even
        // after the user agreed to discard it for that unsuccessful Open.
        command("open_document", "Open…")
        workspaceActivate(dialog.buttons["Cancel"])
        XCTAssertTrue(dialog.waitForNonExistence(timeout: 10))
        expectPixels(paper)
        command("open_document", "Open…")
        workspaceActivate(dialog.buttons["Discard Changes"])
        chooseOpen(project)
        expectPixels(painted)
        XCTAssertEqual(titleText(), savedTitle); XCTAssertEqual(rows.count, savedRows)
        XCTAssertEqual(try Data(contentsOf: project), saved)
        XCTAssertEqual(try Data(contentsOf: invalid), invalidBytes)
        XCTAssertFalse(dialog.exists); XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "failed-open-retry-restores-saved-artwork")
    }

    @MainActor func checkNativeImageImport(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Image " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("Imported blue.png")
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let title = app.staticTexts["document-title"]
        let extent = (title.value as? String ?? title.label).components(separatedBy: " · ").last!
        let size = extent.components(separatedBy: " × ").compactMap(Int.init)
        XCTAssertEqual(size.count, 2)
        let context = try XCTUnwrap(CGContext(data: nil, width: size[0], height: size[1], bitsPerComponent: 8,
            bytesPerRow: size[0] * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(red: 0.1, green: 0.3, blue: 0.9, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: size[0], height: size[1]))
        let output = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(output, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(output))
        let source = try Data(contentsOf: url)
        let second = root.appendingPathComponent("Second blue.png")
        try source.write(to: second)
        let layers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let importImage = app.buttons["layer-Import image as layer"]
        let open = app.windows.buttons["OKButton"].firstMatch
        let paper = editorPixels(in: app)
        workspaceActivate(importImage)
        XCTAssertTrue(open.waitForExistence(timeout: 15))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(open.waitForNonExistence(timeout: 10))
        XCTAssertEqual(layers.count, 2)
        XCTAssertEqual(editorPixels(in: app), paper, "Cancelling image selection must preserve the drawing")
        func chooseBatch() {
            workspaceActivate(importImage)
            XCTAssertTrue(open.waitForExistence(timeout: 15))
            // Go to the first file, which selects it in the native browser.
            // Column-view filenames are editable text fields, not static labels.
            app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(url.path + "\n")
            expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: open)
            waitForExpectations(timeout: 15)
            app.typeKey("a", modifierFlags: .command); workspaceActivate(open)
            XCTAssertTrue(open.waitForNonExistence(timeout: 15))
            expectation(for: NSPredicate(format: "count == 4"), evaluatedWith: layers)
            expectation(for: NSPredicate { _, _ in
                let pixels = self.editorPixels(in: app); return Int(pixels[2]) > Int(pixels[0]) + 100
            }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
        }
        chooseBatch()
        let apply = app.buttons["photo-placement-apply"], cancel = app.buttons["photo-placement-cancel"]
        XCTAssertTrue(apply.waitForExistence(timeout: 10)); XCTAssertTrue(cancel.isHittable)
        editorMenu(in: app, menu: "View", id: "zen_mode", label: "Zen mode")
        XCTAssertTrue(apply.isHittable); XCTAssertTrue(cancel.isHittable)
        attachEditor(in: app, name: "native-photo-placement-panels-hidden")
        workspaceActivate(cancel)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10))
        editorMenu(in: app, menu: "View", id: "zen_mode", label: "Zen mode")
        expectation(for: NSPredicate(format: "count == 2"), evaluatedWith: layers)
        waitForExpectations(timeout: 15)
        XCTAssertEqual(editorPixels(in: app), paper, "Cancel removes the complete provisional batch")
        chooseBatch()
        workspaceActivate(app.buttons["photo-placement-original-size"])
        workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts[url.deletingPathExtension().lastPathComponent].firstMatch.exists, "Use the selected photo name for its layer")
        let imported = editorPixels(in: app)
        attachEditor(in: app, name: "native-image-imported")
        for (command, count, expected) in [("Undo", 2, paper), ("Redo", 4, imported)] {
            editorHistory(command, in: app)
            expectation(for: NSPredicate { _, _ in
                layers.count == count && self.editorPixels(in: app) == expected
            }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        XCTAssertEqual(try Data(contentsOf: url), source)
        XCTAssertEqual(try Data(contentsOf: second), source)
        XCTAssertFalse(app.alerts.firstMatch.exists); XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "native-image-redone")
    }

    @MainActor func checkNativeProjectRoundTrip(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Files " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let project = root.appendingPathComponent("RoundTrip.capy")
        let png = root.appendingPathComponent("Before.png")
        let reopenedPNG = root.appendingPathComponent("After.png")
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let title = app.staticTexts["document-title"]
        func titleText() -> String { title.value as? String ?? title.label }
        let extent = titleText().components(separatedBy: " · ").last!
        let dimensions = extent.components(separatedBy: " × ").compactMap(Int.init)
        XCTAssertEqual(dimensions.count, 2)
        let layers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        func command(_ id: String, _ label: String) {
            editorMenu(in: app, menu: "File", id: id, label: label)
        }
        func goTo(_ url: URL) {
            app.typeKey("g", modifierFlags: [.command, .shift])
            app.typeText(url.path + "\n")
        }
        func savePanel(to url: URL) {
            let save = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(save.waitForExistence(timeout: 15))
            goTo(root)
            let name = app.textFields["saveAsNameTextField"]
            workspaceActivate(name)
            name.typeKey("a", modifierFlags: .command); name.typeText(url.lastPathComponent)
            workspaceActivate(save)
            XCTAssertTrue(save.waitForNonExistence(timeout: 15))
            expectation(for: NSPredicate { _, _ in FileManager.default.fileExists(atPath: url.path) }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func export(to url: URL) {
            command("export_document", "Export…"); chooseExportDestination(in: app); savePanel(to: url)
            XCTAssertTrue(titleText().hasPrefix("RoundTrip.capy · "), "PNG export must retain the editable project location")
        }
        func pixels(_ url: URL) throws -> Data {
            let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
            let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
            XCTAssertEqual(image.width, dimensions[0]); XCTAssertEqual(image.height, dimensions[1])
            var data = Data(count: image.width * image.height * 4)
            data.withUnsafeMutableBytes { bytes in
                let context = CGContext(data: bytes.baseAddress, width: image.width, height: image.height,
                    bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            }
            return data
        }
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app); return Int(sample[2]) > Int(sample[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let artwork = editorPixels(in: app)
        command("save_document_as", "Save As…"); savePanel(to: project)
        let firstSave = try Data(contentsOf: project)
        editorMenu(in: app, menu: "Layer", id: "add_layer", label: "New layer")
        expectation(for: NSPredicate(format: "count == 3"), evaluatedWith: layers)
        waitForExpectations(timeout: 10)
        command("save_document", "Save")
        expectation(for: NSPredicate { _, _ in (try? Data(contentsOf: project)) != firstSave }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let saved = try Data(contentsOf: project)
        export(to: png)
        let before = try pixels(png)
        var allBlue = true
        for offset in stride(from: 0, to: before.count, by: 4) {
            let red = Int(before[offset]), blue = Int(before[offset + 2])
            if blue <= red + 50 || before[offset + 3] != 255 { allBlue = false; break }
        }
        XCTAssertTrue(allBlue, "The exported image must contain the opaque blue artwork")
        attachEditor(in: app, name: "native-project-saved")
        app.terminate(); app.launch()
        XCTAssertTrue(title.waitForExistence(timeout: 30))
        XCTAssertTrue(titleText().hasPrefix("Untitled · "))
        XCTAssertEqual(layers.count, 2, "Reopen must load the saved file, independently of in-memory artwork")
        command("open_document", "Open…")
        let open = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 15))
        goTo(project); workspaceActivate(open)
        XCTAssertTrue(open.waitForNonExistence(timeout: 15))
        expectation(for: NSPredicate { _, _ in titleText().hasPrefix("RoundTrip.capy · ") }, evaluatedWith: app)
        expectation(for: NSPredicate(format: "count == 3"), evaluatedWith: layers)
        waitForExpectations(timeout: 30)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == artwork }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        export(to: reopenedPNG)
        XCTAssertEqual(try pixels(reopenedPNG), before, "Every decoded export pixel must survive project save/reopen")
        XCTAssertEqual(try Data(contentsOf: project), saved, "Opening and exporting must preserve the editable project bytes")
        XCTAssertFalse(app.alerts.firstMatch.exists); XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "native-project-reopened-and-exported")
    }
}
#endif

extension XCTestCase {
    @MainActor func checkNewDrawingAndExportCancellation(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 15))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        func replace(_ field: XCUIElement, _ text: String) {
            activate(field)
            #if os(macOS)
            field.typeKey("a", modifierFlags: .command)
            field.typeText(text)
            #else
            let count = (field.value as? String)?.count ?? 0
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + text)
            #endif
        }
        let width = app.textFields["new-document-width"]
        let height = app.textFields["new-document-height"]
        let create = app.buttons["new-document-create"]
        replace(width, "0")
        XCTAssertFalse(create.isEnabled, "Invalid dimensions must not allocate a canvas")
        replace(width, "63"); replace(height, "47")
        XCTAssertEqual(width.value as? String, "63")
        XCTAssertEqual(height.value as? String, "47")
        activate(create)
        #if os(iOS)
        // The floating number pad consumes the outside tap. If the form remains
        // after dismissal settles, activate Create with the pad out of the way.
        if !create.waitForNonExistence(timeout: 2) { activate(create) }
        #endif
        let title = app.staticTexts["document-title"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Untitled · 63 × 47", "Untitled · 63 × 47"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        #if os(macOS)
        // Exercise the editor action through its shortcut, never system-menu coordinates.
        app.typeKey("e", modifierFlags: [.command, .shift])
        #else
        activate(app.descendants(matching: .any)["menu-File"].firstMatch)
        activate(app.buttons["command-export_document"])
        #endif
        chooseExportDestination(in: app)
        // Files also exposes a non-button Cancel element with stale bounds.
        // Target the native close button shown by the unlocked picker.
        let cancel = app.buttons["Cancel"].firstMatch
        #if os(macOS)
        XCTAssertTrue(cancel.waitForExistence(timeout: 15))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        #else
        activate(cancel)
        #endif
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: cancel)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(title.label == "Untitled · 63 × 47" || title.value as? String == "Untitled · 63 × 47",
            "Export cancellation must retain the drawing")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(iOS)
        activate(app.descendants(matching: .any)["menu-File"].firstMatch)
        activate(app.buttons["command-close_document"])
        expectation(for: NSPredicate { _, _ in
            [.runningBackground, .runningBackgroundSuspended, .notRunning].contains(app.state)
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        #endif
    }
}
