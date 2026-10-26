import XCTest
import ImageIO
import UniformTypeIdentifiers

#if os(macOS)
import AppKit

/// Keep user clipboard data in memory and never overwrite a newer user copy.
@MainActor final class NativePhotoPasteboard {
    let board = NSPasteboard.general
    let saved: [NSPasteboardItem]
    var changeCount: Int
    var replaced = false

    init() throws {
        changeCount = board.changeCount
        saved = try (board.pasteboardItems ?? []).map { item in
            let copy = NSPasteboardItem()
            for type in item.types {
                let data = try XCTUnwrap(item.data(forType: type), "Preserve clipboard before testing")
                XCTAssertTrue(copy.setData(data, forType: type))
            }
            return copy
        }
        XCTAssertEqual(board.changeCount, changeCount)
    }
    func replace(_ items: [NSPasteboardItem]) throws {
        guard board.changeCount == changeCount else {
            throw NSError(domain: "PhotoClipboardCheck", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Clipboard changed during the test"])
        }
        board.prepareForNewContents(with: .currentHostOnly)
        replaced = true
        let written = board.writeObjects(items)
        changeCount = board.changeCount
        XCTAssertTrue(written)
    }
    func restore() {
        guard replaced && board.changeCount == changeCount else { return }
        board.prepareForNewContents(with: .currentHostOnly)
        if !saved.isEmpty { XCTAssertTrue(board.writeObjects(saved)) }
    }
}

extension XCTestCase {
    @MainActor func fileCommand(_ id: String, _ label: String, in app: XCUIApplication) {
        editorMenu(in: app, menu: "File", id: id, label: label)
    }
    @MainActor func goToFolder(_ url: URL, in app: XCUIApplication) {
        app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(url.path + "\n")
    }
    @MainActor func checkSavePanelKeepsItsDocumentAcrossWindowFocus(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Window Files " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let firstURL = root.appendingPathComponent("First.capy")
        let secondURL = root.appendingPathComponent("Second.capy")
        let copyURL = root.appendingPathComponent("First Copy.capy")
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch()
        let paint = app.buttons["workspace-switch-builtin:workspace:illustrator"]
        XCTAssertTrue(paint.waitForExistence(timeout: 30))
        if !paint.isSelected { workspaceActivate(paint) }
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"),
            evaluatedWith: app.descendants(matching: .any)["canvas"].firstMatch)
        waitForExpectations(timeout: 30)
        let scenes = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-"))
        let firstID = scenes.firstMatch.identifier
        let first = app.windows.containing(.any, identifier: firstID).firstMatch
        func expectTitle(_ window: XCUIElement, _ name: String) {
            expectation(for: NSPredicate { _, _ in window.title == name }, evaluatedWith: window)
                .expectationDescription = "Native window title: \(name)"
            waitForExpectations(timeout: 15)
        }
        func focus(_ name: String) {
            let bar = app.menuBars.menuBarItems["Window"]
            workspaceActivate(bar)
            let item = bar.descendants(matching: .menuItem)[name].firstMatch
            XCTAssertTrue(item.waitForExistence(timeout: 5))
            let screen = XCUIScreen.main.screenshot().image.size
            let origin = first.frame.origin
            let scroll = first.coordinate(withNormalizedOffset: .zero)
                .withOffset(CGVector(dx: item.frame.midX - origin.x, dy: screen.height - 4 - origin.y))
            for _ in 0..<20 where !item.isHittable { scroll.hover(); Thread.sleep(forTimeInterval: 0.3) }
            item.click()
        }
        func expectRows(_ window: XCUIElement, _ count: Int) {
            let rows = window.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            expectation(for: NSPredicate { _, _ in rows.count == count }, evaluatedWith: window)
                .expectationDescription = "\(count) visible layer rows in \(window.title)"
            waitForExpectations(timeout: 15)
        }
        func beginSaveAs(_ url: URL) -> XCUIElement {
            fileCommand("save_document_as", "Save As…", in: app)
            let save = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(save.waitForExistence(timeout: 15))
            app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(root.path + "\n")
            let name = app.textFields["saveAsNameTextField"]
            workspaceActivate(name)
            name.typeKey("a", modifierFlags: .command); name.typeText(url.lastPathComponent)
            return save
        }
        func finishSave(_ save: XCUIElement) {
            XCTAssertTrue(save.isHittable)
            save.click()
            XCTAssertTrue(save.waitForNonExistence(timeout: 15))
        }
        func history(_ label: String) {
            editorMenu(in: app, menu: "Edit", id: label.lowercased(), label: label)
        }
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app); return Int(sample[2]) > Int(sample[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let painted = editorPixels(in: app)
        finishSave(beginSaveAs(firstURL))
        expectTitle(first, "First.capy")
        let originalFirst = try Data(contentsOf: firstURL)
        fileCommand("new_window", "New Window", in: app)
        let secondScene = scenes.matching(NSPredicate(format: "identifier != %@", firstID)).firstMatch
        XCTAssertTrue(secondScene.waitForExistence(timeout: 30))
        // Paint belongs to the first window; use the available Photo workspace
        // to expose the second drawing's Layers panel without moving that owner.
        workspaceActivate(secondScene.buttons["workspace-switch-builtin:workspace:photographer"])
        let second = app.windows.containing(.any, identifier: secondScene.identifier).firstMatch
        XCTAssertTrue(second.waitForExistence(timeout: 15), app.debugDescription)
        expectRows(second, 2)
        finishSave(beginSaveAs(secondURL))
        expectTitle(second, "Second.capy")
        let originalSecond = try Data(contentsOf: secondURL)

        focus("First.capy")
        workspaceActivate(first.buttons["layer-New layer"])
        expectRows(first, 3)
        _ = beginSaveAs(copyURL)
        focus("Second.capy")
        workspaceActivate(second.buttons["layer-New layer"])
        expectRows(second, 3)
        history("Undo"); expectRows(second, 2)
        history("Redo"); expectRows(second, 3)
        focus("First.capy")
        let cancel = app.windows.buttons["CancelButton"].firstMatch
        XCTAssertTrue(cancel.isHittable)
        cancel.click()
        XCTAssertTrue(app.windows.buttons["OKButton"].firstMatch.waitForNonExistence(timeout: 15))
        XCTAssertFalse(FileManager.default.fileExists(atPath: copyURL.path))
        expectTitle(first, "First.capy"); expectTitle(second, "Second.capy")
        expectRows(first, 3); expectRows(second, 3)
        XCTAssertEqual(try Data(contentsOf: firstURL), originalFirst)
        XCTAssertEqual(try Data(contentsOf: secondURL), originalSecond)

        focus("First.capy")
        history("Undo"); expectRows(first, 2)
        history("Redo"); expectRows(first, 3)
        let retry = beginSaveAs(copyURL)
        focus("Second.capy")
        focus("First.capy")
        finishSave(retry)
        expectTitle(first, "First Copy.capy"); expectTitle(second, "Second.capy")
        let copied = try Data(contentsOf: copyURL)
        XCTAssertEqual(try Data(contentsOf: firstURL), originalFirst)
        XCTAssertEqual(try Data(contentsOf: secondURL), originalSecond)
        focus("Second.capy")
        fileCommand("save_document", "Save", in: app)
        expectation(for: NSPredicate { _, _ in (try? Data(contentsOf: secondURL)) != originalSecond }, evaluatedWith: second)
        waitForExpectations(timeout: 15)
        XCTAssertEqual(try Data(contentsOf: copyURL), copied)

        focus("First Copy.capy")
        fileCommand("open_document", "Open…", in: app)
        let open = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 15))
        app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(copyURL.path + "\n")
        workspaceActivate(open)
        XCTAssertTrue(open.waitForNonExistence(timeout: 15))
        expectTitle(first, "First Copy.capy"); expectRows(first, 3)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == painted }, evaluatedWith: first)
        waitForExpectations(timeout: 15)
        focus("Second.capy")
        history("Undo"); expectRows(second, 2)
        expectRows(first, 3)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        XCTAssertFalse(app.sheets.firstMatch.exists)
        attachEditor(in: app, name: "independent-file-dialog-owners")
        app.terminate()
    }

    @MainActor func checkNativeImagePaste(in app: XCUIApplication) throws {
        let clipboard = try NativePhotoPasteboard()
        addTeardownBlock { await MainActor.run { clipboard.restore() } }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Paste " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let photo = root.appendingPathComponent("Clipboard blue.png")
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let title = editorDocumentTitle(in: app)
        let extent = (title.value as? String ?? title.label).components(separatedBy: " · ").last!
        let size = extent.components(separatedBy: " × ").compactMap(Int.init)
        XCTAssertEqual(size.count, 2)
        let context = try XCTUnwrap(CGContext(data: nil, width: size[0], height: size[1], bitsPerComponent: 8,
            bytesPerRow: size[0] * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(red: 0.1, green: 0.3, blue: 0.9, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: size[0], height: size[1]))
        let output = try XCTUnwrap(CGImageDestinationCreateWithURL(photo as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(output, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(output))
        let original = try Data(contentsOf: photo)
        func item(_ data: Data, type: NSPasteboard.PasteboardType = .png) -> NSPasteboardItem {
            let item = NSPasteboardItem(); XCTAssertTrue(item.setData(data, forType: type)); return item
        }
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let apply = app.buttons["canvas-bar-action-apply_transform"]
        func paste() { editorMenu(in: app, menu: "Edit", id: "paste_image", label: "Paste") }
        func expect(_ count: Int, _ pixels: Data) {
            expectation(for: NSPredicate { _, _ in
                rows.count == count && self.editorPixels(in: app) == pixels
            }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        let paper = editorPixels(in: app)
        try clipboard.replace([item(original), item(original)])
        paste()
        let pasted = app.buttons["drawing-tab-2"].firstMatch, startup = app.buttons["drawing-tab-1"].firstMatch
        if !pasted.waitForExistence(timeout: 5) { workspaceActivate(app.buttons["document-title"]) }
        XCTAssertTrue(pasted.waitForExistence(timeout: 20), "Paste on the untouched startup canvas opens a new drawing")
        XCTAssertTrue(pasted.isSelected); XCTAssertTrue((pasted.value as? String ?? "").contains(extent))
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return rows.count == 3 && Int(pixel[2]) > Int(pixel[0]) + 100
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        XCTAssertFalse(apply.exists)
        attachEditor(in: app, name: "native-clipboard-startup-new-drawing")
        if !startup.exists { workspaceActivate(app.buttons["document-title"]) }
        workspaceActivate(startup)
        expect(2, paper)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        for cancel in [true, false] {
            try clipboard.replace([item(original), item(original)])
            paste(); XCTAssertTrue(apply.waitForExistence(timeout: 20)); XCTAssertEqual(rows.count, 4)
            workspaceActivate(cancel ? app.buttons["canvas-bar-action-cancel_transform"] : apply)
            XCTAssertTrue(apply.waitForNonExistence(timeout: 10))
            if cancel { expect(2, paper) }
        }
        let painted = editorPixels(in: app)
        XCTAssertGreaterThan(Int(painted[2]), Int(painted[0]) + 100)
        for (command, count, pixels) in [("Undo", 2, paper), ("Redo", 4, painted)] {
            editorHistory(command, in: app); expect(count, pixels)
        }
        try clipboard.replace([item(original), item(Data("invalid PNG".utf8))])
        paste()
        let failure = app.sheets.firstMatch
        XCTAssertTrue(failure.waitForExistence(timeout: 20))
        workspaceActivate(failure.buttons["OK"])
        XCTAssertTrue(failure.waitForNonExistence(timeout: 10))
        XCTAssertFalse(apply.exists); expect(4, painted)
        // A failed batch must not insert its first valid member or add history.
        for (command, count, pixels) in [("Undo", 2, paper), ("Redo", 4, painted)] {
            editorHistory(command, in: app); expect(count, pixels)
        }
        try clipboard.replace([item(photo.dataRepresentation, type: .fileURL)])
        paste(); XCTAssertTrue(apply.waitForExistence(timeout: 20)); XCTAssertEqual(rows.count, 5, "A pasted file creates its own Object layer")
        workspaceActivate(app.buttons["canvas-bar-action-cancel_transform"])
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10)); expect(4, painted)
        XCTAssertEqual(try Data(contentsOf: photo), original)
        XCTAssertFalse(app.sheets.firstMatch.exists)
        attachEditor(in: app, name: "native-clipboard-batch-history-and-failure-retry")
    }

    @MainActor func checkNativeImageDrop(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Drop " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let photo = root.appendingPathComponent("Dropped blue.png")
        let context = try XCTUnwrap(CGContext(data: nil, width: 64, height: 48, bitsPerComponent: 8,
            bytesPerRow: 256, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(red: 0.1, green: 0.3, blue: 0.9, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: 64, height: 48))
        let output = try XCTUnwrap(CGImageDestinationCreateWithURL(photo as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(output, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(output))
        let original = try Data(contentsOf: photo)
        app.launch(); capturePaintEditor(in: app)
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        XCTAssertEqual(rows.count, 2)
        let fixture = Bundle.main.bundleURL.deletingLastPathComponent().appendingPathComponent("PhotoDragSource.app")
        XCTAssertTrue(FileManager.default.fileExists(atPath: fixture.path), "The Mac UI-test build must include its native drag source")
        let donor = XCUIApplication(url: fixture)
        donor.launchArguments = [photo.path, String(canvas.frame.minX + 20), String(canvas.frame.minY + 100)]
        donor.launch()
        defer { if donor.state != .notRunning { donor.terminate() } }
        let sourceView = donor.descendants(matching: .any)["photo-drag-source"].firstMatch
        XCTAssertTrue(sourceView.waitForExistence(timeout: 10))
        var starts = 0
        func drag(to destination: XCUICoordinate) {
            donor.activate()
            sourceView.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                .press(forDuration: 0.1, thenDragTo: destination)
            starts += 1
            XCTAssertEqual(sourceView.value as? String, String(starts), "The external source must receive the native drag")
        }
        let apply = app.buttons["canvas-bar-action-apply_transform"]
        drag(to: canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)))
        XCTAssertTrue(apply.waitForExistence(timeout: 20), "A native external canvas drop must start placement")
        XCTAssertEqual(rows.count, 3)
        attachEditor(in: app, name: "native-canvas-photo-drop")
        workspaceActivate(app.buttons["canvas-bar-action-cancel_transform"])
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10)); XCTAssertEqual(rows.count, 2)
        let paper = app.descendants(matching: .any)["layer-row-2"].firstMatch
        XCTAssertTrue(paper.isHittable)
        drag(to: paper.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.5)))
        XCTAssertTrue(apply.waitForExistence(timeout: 20), "A native external row drop must start placement")
        workspaceActivate(apply); XCTAssertTrue(apply.waitForNonExistence(timeout: 10))
        XCTAssertEqual(rows.count, 3)
        // AppKit can expose this file as PNG bytes with no suggested name.
        // Supplied names are checked separately through the provider owner test.
        XCTAssertTrue(app.staticTexts["Dropped blue"].firstMatch.waitForExistence(timeout: 10) || app.staticTexts["Imported image"].firstMatch.exists)
        for (command, count) in [("Undo", 2), ("Redo", 3)] {
            editorHistory(command, in: app)
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: rows)
            waitForExpectations(timeout: 10)
        }
        XCTAssertEqual(try Data(contentsOf: photo), original)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        attachEditor(in: app, name: "native-layer-photo-drop-redone")
    }

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
        let title = editorDocumentTitle(in: app)
        func titleText() -> String { title.value as? String ?? title.label }
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        func chooseOpen(_ url: URL) {
            let open = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(open.waitForExistence(timeout: 15))
            goToFolder(url, in: app); workspaceActivate(open)
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
        fileCommand("save_document_as", "Save As…", in: app)
        let save = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(save.waitForExistence(timeout: 15)); goToFolder(root, in: app)
        let name = app.textFields["saveAsNameTextField"]
        workspaceActivate(name)
        name.typeKey("a", modifierFlags: .command); name.typeText(project.lastPathComponent)
        workspaceActivate(save)
        XCTAssertTrue(save.waitForNonExistence(timeout: 15))
        app.activate()
        expectation(for: NSPredicate { _, _ in titleText().hasPrefix("Survivor.capy · ") }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let saved = try Data(contentsOf: project)
        let savedTitle = titleText(), savedRows = rows.count
        // SwiftUI presents these native Mac alerts as sheets. Scope their
        // buttons to that sheet, excluding the Touch Bar's duplicate actions.
        let dialog = app.sheets.firstMatch

        editorMenu(in: app, menu: "Layer", id: "clear_layer", label: "Clear Entire Layer")
        expectPixels(paper, in: app)
        fileCommand("open_document", "Open…", in: app)
        chooseOpen(invalid)
        let failure = dialog
        XCTAssertTrue(failure.waitForExistence(timeout: 20), "An invalid project must report its error")
        XCTAssertTrue(failure.staticTexts["Document"].exists)
        XCTAssertGreaterThan(failure.staticTexts.count, 1, "The error must explain why the project could not open")
        workspaceActivate(failure.buttons["OK"])
        XCTAssertTrue(failure.waitForNonExistence(timeout: 10))
        XCTAssertEqual(titleText(), savedTitle); XCTAssertEqual(rows.count, savedRows)
        expectPixels(paper, in: app)
        for (action, pixels) in [("Undo", painted), ("Redo", paper)] {
            editorHistory(action, in: app); expectPixels(pixels, in: app)
        }
        attachEditor(in: app, name: "failed-open-preserves-unsaved-artwork-and-history")

        // Open now adds a drawing. Cancelling its native picker must preserve
        // the original dirty owner and its history without a discard decision.
        fileCommand("open_document", "Open…", in: app)
        let picker = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(picker.waitForExistence(timeout: 15))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
        XCTAssertTrue(dialog.waitForNonExistence(timeout: 10))
        expectPixels(paper, in: app)
        fileCommand("open_document", "Open…", in: app)
        chooseOpen(project)
        expectPixels(painted, in: app)
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
        let title = editorDocumentTitle(in: app)
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
            expectation(for: NSPredicate(format: "count == 3"), evaluatedWith: layers)
            expectation(for: NSPredicate { _, _ in
                let pixels = self.editorPixels(in: app); return Int(pixels[2]) > Int(pixels[0]) + 100
            }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
        }
        chooseBatch()
        let apply = app.buttons["canvas-bar-action-apply_transform"], cancel = app.buttons["canvas-bar-action-cancel_transform"]
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
        activateCanvasBarAction("placement_original_size", label: "Original Size (100%)", in: app)
        workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts[url.deletingPathExtension().lastPathComponent].firstMatch.waitForExistence(timeout: 10),
            "Use the selected photo name for its Object layer")
        let imported = editorPixels(in: app)
        attachEditor(in: app, name: "native-image-imported")
        for (command, count, expected) in [("Undo", 2, paper), ("Redo", 3, imported)] {
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
        let title = editorDocumentTitle(in: app)
        func titleText() -> String { title.value as? String ?? title.label }
        let extent = titleText().components(separatedBy: " · ").last!
        let dimensions = extent.components(separatedBy: " × ").compactMap(Int.init)
        XCTAssertEqual(dimensions.count, 2)
        let layers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        func savePanel(to url: URL) {
            let save = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(save.waitForExistence(timeout: 15))
            goToFolder(root, in: app)
            let name = app.textFields["saveAsNameTextField"]
            workspaceActivate(name)
            name.typeKey("a", modifierFlags: .command); name.typeText(url.lastPathComponent)
            workspaceActivate(save)
            XCTAssertTrue(save.waitForNonExistence(timeout: 15))
            expectation(for: NSPredicate { _, _ in FileManager.default.fileExists(atPath: url.path) }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func export(to url: URL) {
            fileCommand("export_document", "Export…", in: app); chooseExportDestination(in: app); savePanel(to: url)
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
        fileCommand("save_document_as", "Save As…", in: app); savePanel(to: project)
        let firstSave = try Data(contentsOf: project)
        editorMenu(in: app, menu: "Layer", id: "add_layer", label: "New layer", submenu: "New")
        expectation(for: NSPredicate(format: "count == 3"), evaluatedWith: layers)
        waitForExpectations(timeout: 10)
        fileCommand("save_document", "Save", in: app)
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
        app.terminate(); useFreshStorage(app); app.launch()
        XCTAssertTrue(title.waitForExistence(timeout: 30))
        XCTAssertTrue(titleText().hasPrefix("Untitled · "))
        XCTAssertEqual(layers.count, 2, "Reopen must load the saved file, independently of in-memory artwork")
        fileCommand("open_document", "Open…", in: app)
        let open = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 15))
        goToFolder(project, in: app); workspaceActivate(open)
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
            element.clickOrTap()
        }
        let create = app.buttons["new-drawing-create"], resolution = newDrawingNumber("resolution", in: app)
        XCTAssertTrue(resolution.waitForExistence(timeout: 15))
        #if os(iOS)
        let initialTitle = editorDocumentTitle(in: app)
        let initialDescription = initialTitle.value as? String ?? initialTitle.label
        XCTAssertFalse(initialDescription.isEmpty)
        #endif
        XCTAssertEqual(resolution.value as? String, "300")
        let form = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        form.name = "new-drawing-resolution"; form.lifetime = .keepAlways; add(form)
        enterNewDrawingNumber("resolution", "600", in: app)
        expectation(for: NSPredicate(format: "value == %@", "600"), evaluatedWith: resolution)
        waitForExpectations(timeout: 15)
        activate(app.buttons["new-drawing-presets-button"])
        let select = app.buttons["new-drawing-select"]
        activate(app.buttons["new-drawing-preset-0"])
        activate(select)
        XCTAssertTrue(select.waitForNonExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "300"), evaluatedWith: resolution)
        waitForExpectations(timeout: 15)
        enterNewDrawingNumber("resolution", "10000", in: app)
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: create)
        waitForExpectations(timeout: 15)
        enterNewDrawingNumber("resolution", "600", in: app)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: create)
        waitForExpectations(timeout: 15)
        let blending = app.buttons["new-drawing-blending"]
        XCTAssertTrue(blending.waitForExistence(timeout: 15), "New drawing offers Blending")
        XCTAssertTrue(blending.isEnabled)
        chooseNewDrawingOption("new-drawing-depth", 2, in: app)
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: blending)
        waitForExpectations(timeout: 10)
        chooseNewDrawingOption("new-drawing-depth", 0, in: app)
        chooseNewDrawingOption("new-drawing-unit", 3, in: app)
        enterNewDrawingNumber("width", "63", in: app)
        enterNewDrawingNumber("height", "47", in: app)
        XCTAssertEqual(newDrawingNumber("width", in: app).value as? String, "63")
        XCTAssertEqual(newDrawingNumber("height", in: app).value as? String, "47")
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        XCTAssertTrue(create.isHittable, create.debugDescription)
        #endif
        activate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 30))
        let title = editorDocumentTitle(in: app)
        expectation(for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "63 × 47", "63 × 47"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        let created = editorText(title)
        editorMenu(in: app, menu: "Image", id: "image_size", label: "Image Size…")
        let ppi = app.textFields["number-entry-image-size-resolution"], sizeApply = app.buttons["image-size-apply"]
        XCTAssertTrue(ppi.waitForExistence(timeout: 15))
        XCTAssertEqual(ppi.value as? String, "600 ppi")
        #if os(iOS)
        XCTAssertTrue(app.staticTexts.matching(identifier: "image-size-resample")
            .matching(NSPredicate(format: "label == %@", "Resample")).firstMatch.exists)
        #endif
        XCTAssertFalse(sizeApply.isEnabled, "Inspecting the created resolution has no pending document edit")
        let size = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        size.name = "created-drawing-resolution"; size.lifetime = .keepAlways; add(size)
        activate(app.buttons["image-size-cancel"])
        XCTAssertTrue(sizeApply.waitForNonExistence(timeout: 10))
        editorMenu(in: app, menu: "File", id: "export_document", label: "Export…")
        #if os(iOS)
        let preview = app.buttons["export-preview"]
        XCTAssertTrue(preview.waitForExistence(timeout: 15))
        XCTAssertEqual(preview.label, "Preview Output")
        let exportScroll = app.scrollViews.containing(.button, identifier: "export-preview").firstMatch
        let fit = app.switches["export-fit"]
        revealEditorControl(fit, in: exportScroll)
        if fit.value as? String != "1" { workspaceActivate(fit) }
        let exportWidth = app.textFields["export-width"], exportHeight = app.textFields["export-height"]
        replaceEditorText(exportWidth, with: "7.5", in: app)
        dismissEditorKeyboard(in: app)
        revealEditorControl(preview, in: exportScroll); workspaceActivate(preview)
        let integerError = app.staticTexts["export-error"]
        expectation(for: NSPredicate { _, _ in
            integerError.exists && (integerError.label == "Maximum width (px) needs a whole number." || integerError.value as? String == "Maximum width (px) needs a whole number.")
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        XCTAssertEqual(exportWidth.value as? String, "7.5")
        replaceEditorText(exportWidth, with: "32", in: app)
        replaceEditorText(exportHeight, with: "32", in: app)
        dismissEditorKeyboard(in: app)
        revealEditorControl(preview, in: exportScroll); workspaceActivate(preview)
        let outputSize = app.staticTexts["export-output-size"]
        expectation(for: NSPredicate { _, _ in
            outputSize.exists && (outputSize.label == "Output: 32 × 24 px" || outputSize.value as? String == "Output: 32 × 24 px")
        }, evaluatedWith: app)
        waitForExpectations(timeout: 60)
        #endif
        chooseExportDestination(in: app)
        // Files also exposes a non-button Cancel element with stale bounds.
        // Target the native close button shown by the unlocked picker.
        #if os(macOS)
        let cancel = app.windows.buttons["CancelButton"].firstMatch
        let commit = app.windows.buttons["OKButton"].firstMatch
        expectation(for: NSPredicate { _, _ in cancel.exists && commit.exists }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        expectation(for: NSPredicate { _, _ in !cancel.exists && !commit.exists }, evaluatedWith: app)
        #else
        let cancel = app.buttons["Cancel"].firstMatch
        activate(cancel)
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: cancel)
        #endif
        waitForExpectations(timeout: 10)
        XCTAssertEqual(editorText(title), created, "Export cancellation must retain the drawing")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(iOS)
        editorMenu(in: app, menu: "File", id: "close_document", label: "Close")
        expectation(for: NSPredicate { _, _ in
            let restored = self.editorDocumentTitle(in: app)
            return app.state == .runningForeground &&
                (restored.label == initialDescription || restored.value as? String == initialDescription)
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        #endif
    }
}


extension XCTestCase {
    @MainActor func checkPreservedPackageCopyOriginal(in app: XCUIApplication, theme: String) async throws {
        let token = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_FILE_TEST_TOKEN"])
        guard UUID(uuidString: token) != nil else { XCTFail("A private fixture UUID is required"); return }
        let folder = "Capy Files " + token
        #if os(macOS)
        let directory = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_PACKAGE_DIRECTORY"])
        let root = URL(fileURLWithPath: directory, isDirectory: true)
        guard directory.hasPrefix("/"), root.lastPathComponent == folder else { XCTFail("Private fixture directory mismatch"); return }
        #else
        let root = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent(folder, isDirectory: true)
        let provider = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_PACKAGE_PROVIDER"])
        guard provider.hasPrefix("Capy Test Files ") else { XCTFail("Private runner provider mismatch"); return }
        #endif
        let source = root.appendingPathComponent("Unsupported.capy"), destination = root.appendingPathComponent("Copy.capy")
        func bytes(_ url: URL) async throws -> Data {
            try await Task.detached(priority: .utility) {
                let values = try url.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey, .isSymbolicLinkKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true,
                    let count = values.fileSize, (1...65536).contains(count) else { throw CocoaError(.fileReadCorruptFile) }
                return try Data(contentsOf: url)
            }.value
        }
        let existing = try await Task.detached(priority: .utility) {
            try FileManager.default.contentsOfDirectory(atPath: root.path).sorted()
        }.value
        guard existing == [source.lastPathComponent] else { XCTFail("The private fixture folder must contain only its input"); return }
        let original = try await bytes(source)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch()
        let paint = app.buttons["workspace-switch-builtin:workspace:illustrator"]
        XCTAssertTrue(paint.waitForExistence(timeout: 30))
        if !paint.isSelected { workspaceActivate(paint) }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let startup = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", "Canvas ready"), object: canvas)
        await fulfillment(of: [startup], timeout: 30)
        let title = editorDocumentTitle(in: app)
        let originalTitle = title.value as? String ?? title.label
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let originalRows = rows.allElementsBoundByIndex.map(\.identifier).sorted()
        let paper = editorPixels(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        let fill = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            let pixels = self.editorPixels(in: app); return Int(pixels[2]) > Int(pixels[0]) + 50
        }, object: app)
        await fulfillment(of: [fill], timeout: 15)
        let painted = editorPixels(in: app)
        #if os(iOS)
        func folderTitle(_ value: String) -> XCUIElement {
            app.otherElements["DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: " + value]
        }
        func enterRunnerFolder() {
            let local = app.cells["DOC.sidebar.item.On My iPad"]
            XCTAssertTrue(local.waitForExistence(timeout: 15)); workspaceActivate(local)
            XCTAssertTrue(folderTitle("On My iPad").waitForExistence(timeout: 10))
            workspaceActivate(fileProviderRoot(provider, in: app))
            XCTAssertTrue(folderTitle(provider).waitForExistence(timeout: 10))
            workspaceActivate(app.cells[folder + ", Folder"])
            XCTAssertTrue(folderTitle(folder).waitForExistence(timeout: 10))
        }
        #endif
        editorMenu(in: app, menu: "File", id: "open_document", label: "Open…")
        #if os(macOS)
        let open = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 15)); goToFolder(source, in: app); workspaceActivate(open)
        XCTAssertTrue(open.waitForNonExistence(timeout: 20))
        #else
        enterRunnerFolder()
        let input = fileProviderFile("Unsupported.capy", in: app).images.firstMatch
        XCTAssertTrue(input.waitForExistence(timeout: 10)); workspaceActivate(input)
        submitFileProviderSelection(in: app)
        #endif
        let preview = app.descendants(matching: .any)["preserved-package-preview"].firstMatch
        let copy = app.buttons["copy-original-package"]
        XCTAssertTrue(copy.waitForExistence(timeout: 30)); XCTAssertTrue(copy.isEnabled)
        XCTAssertTrue(preview.exists)
        XCTAssertTrue(app.staticTexts["This drawing can be viewed, but cannot be edited."].exists)
        XCTAssertTrue(app.staticTexts["Pass through requires a stack"].exists)
        XCTAssertFalse(app.buttons["export-package-preview"].exists)
        XCTAssertFalse(canvas.isHittable, "The preserved package must not expose editable canvas input")
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(copy.exists)
        attachEditor(in: app, name: "preserved-unsupported-read-only-" + theme)
        let hierarchy = XCTAttachment(string: app.debugDescription)
        hierarchy.name = "preserved-package-hierarchy-" + theme; hierarchy.lifetime = .keepAlways; add(hierarchy)
        workspaceActivate(copy)
        #if os(macOS)
        let save = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(save.waitForExistence(timeout: 15)); goToFolder(root, in: app)
        let name = app.textFields["saveAsNameTextField"]
        workspaceActivate(name); name.typeKey("a", modifierFlags: .command); name.typeText(destination.lastPathComponent)
        workspaceActivate(save); XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        #else
        let controls = documentExportControls(in: app)
        let name = controls.filename
        enterRunnerFolder()
        XCTAssertTrue(["Copy", destination.lastPathComponent].contains(name.value as? String ?? ""))
        let save = controls.commit
        workspaceActivate(save)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        #endif
        XCTAssertTrue(copy.waitForNonExistence(timeout: 20))
        let copied = try await bytes(destination), retained = try await bytes(source)
        XCTAssertEqual(copied, original, "Copy Original must retain every package byte")
        XCTAssertEqual(retained, original, "Copy Original must preserve its input")
        XCTAssertEqual(title.value as? String ?? title.label, originalTitle)
        XCTAssertEqual(rows.allElementsBoundByIndex.map(\.identifier).sorted(), originalRows)
        expectPixels(painted, in: app)
        editorHistory("Undo", in: app); expectPixels(paper, in: app)
        editorHistory("Redo", in: app); expectPixels(painted, in: app)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists); XCTAssertFalse(app.alerts.firstMatch.exists)
        attachEditor(in: app, name: "copied-original-retains-editor-and-history-" + theme)
    }
}
