import XCTest
import ImageIO
import UniformTypeIdentifiers

#if os(macOS)
extension XCTestCase {
    @MainActor func checkNativePhotoCorrections(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Corrections " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let photo = root.appendingPathComponent("Original photo.png"), project = root.appendingPathComponent("Corrections.capy")
        let context = try XCTUnwrap(CGContext(data: nil, width: 128, height: 96, bitsPerComponent: 8,
            bytesPerRow: 128 * 4, space: CGColorSpace(name: CGColorSpace.displayP3)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(red: 0.25, green: 0.4, blue: 0.65, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: 128, height: 96))
        let output = try XCTUnwrap(CGImageDestinationCreateWithURL(photo as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(output, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(output))
        let original = try Data(contentsOf: photo)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        // Keep the isolated test window clear of the unrelated iCloud prompt.
        let window = app.windows.firstMatch
        let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1)).withOffset(CGVector(dx: -2, dy: -2))
        corner.press(forDuration: 0.1, thenDragTo: corner.withOffset(CGVector(dx: -320, dy: 0)))
        expectation(for: NSPredicate { _,_ in window.frame.width < 950 }, evaluatedWith: window)
        waitForExpectations(timeout: 10)
        func open(_ url: URL) {
            editorMenu(in: app, menu: "File", id: "open_document", label: "Open…")
            let button = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(button.waitForExistence(timeout: 15))
            app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(url.path + "\n")
            workspaceActivate(button); XCTAssertTrue(button.waitForNonExistence(timeout: 15))
        }
        func pixels() -> Data { editorPixels(in: app) }
        func changed(_ before: Data) -> Data {
            expectation(for: NSPredicate { _,_ in pixels() != before }, evaluatedWith: app)
            waitForExpectations(timeout: 15); return pixels()
        }
        func reveal(_ element: XCUIElement) {
            revealEditorControl(element, in: app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch)
        }
        func edit(_ key: String, _ value: String) {
            let readout = app.buttons["number-value-property-" + key]
            reveal(readout); workspaceActivate(readout)
            let field = app.textFields["number-entry-property-" + key]
            XCTAssertTrue(field.waitForExistence(timeout: 5)); field.typeText(value + "\n")
            XCTAssertTrue(field.waitForNonExistence(timeout: 5))
        }
        open(photo)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let rows = app.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let initialCount = rows.count
        let corrections = [("Exposure", "exposure", "exposure", "0.75"),
            ("White Balance", "white_balance", "temperature", "25"),
            ("Levels", "levels", "gamma", "0.9"), ("Curves", "curves", "", ""),
            ("Hue", "hue_saturation", "hue", "10"),
            ("Color Balance", "color_balance", "midtones_red", "12")]
        var ids: [String] = []
        for (index, (name, filterID, key, amount)) in corrections.enumerated() {
            let before = pixels()
            workspaceActivate(app.buttons["panel-tab-adjustments"])
            if index > 0 { workspaceActivate(app.buttons["filter-search-toggle"]) }
            workspaceActivate(app.buttons["filter-search-toggle"])
            let search = app.textFields["filter-search"]
            XCTAssertTrue(search.waitForExistence(timeout: 5)); search.typeText(name)
            let filter = app.buttons["adjustment-" + filterID]
            expectation(for: NSPredicate(format: "value == %@", "Preview ready"), evaluatedWith: filter)
            waitForExpectations(timeout: 20); workspaceActivate(filter)
            expectation(for: NSPredicate(format: "count == %d", initialCount + index + 1), evaluatedWith: rows)
            waitForExpectations(timeout: 10)
            let row = rows.element(boundBy: 0), id = row.identifier
            ids.append(id); expectPixels(before, in: app)
            if key.isEmpty {
                let curve = app.descendants(matching: .any)["effect-curve"].firstMatch
                curve.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25)).click()
            } else { edit(key, amount) }
            let edited = changed(before)
            // Exercise the two native editor types once. The fast Metal test
            // covers every correction's reset, bypass, history and local mask.
            if index == 0 || key.isEmpty {
                editorHistory("Undo", in: app); expectPixels(before, in: app)
                editorHistory("Redo", in: app); expectPixels(edited, in: app)
                if key.isEmpty {
                    reveal(app.buttons["curve-reset"]); workspaceActivate(app.buttons["curve-reset"])
                } else {
                    let number = app.buttons["number-value-property-" + key]
                    reveal(number); number.rightClick(); workspaceActivate(app.menuItems["Reset"].firstMatch)
                }
                expectPixels(before, in: app); editorHistory("Undo", in: app); expectPixels(edited, in: app)
                workspaceActivate(row.buttons["Hide layer"]); expectPixels(before, in: app)
                workspaceActivate(row.buttons["Show layer"]); expectPixels(edited, in: app)
            }
            workspaceActivate(app.buttons["layer-Add layer mask"])
            let mask = row.buttons["Edit layer mask"]
            XCTAssertTrue(mask.waitForExistence(timeout: 5))
            if index == 0 {
                mask.rightClick(); workspaceActivate(app.buttons["Invert mask"]); expectPixels(before, in: app)
                editorHistory("Undo", in: app); expectPixels(edited, in: app)
            }
            workspaceActivate(row.buttons["Edit layer content"])
            if index == 1 || index == 5 { attachEditor(in: app, name: "photo-corrections-" + filterID) }
        }
        let corrected = pixels()
        editorMenu(in: app, menu: "File", id: "save_document_as", label: "Save As…")
        let save = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(save.waitForExistence(timeout: 15))
        app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(root.path + "\n")
        let filename = app.textFields["saveAsNameTextField"]
        workspaceActivate(filename); filename.typeKey("a", modifierFlags: .command); filename.typeText(project.lastPathComponent)
        workspaceActivate(save); XCTAssertTrue(save.waitForNonExistence(timeout: 15))
        expectation(for: NSPredicate { _,_ in FileManager.default.fileExists(atPath: project.path) }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        open(project); expectPixels(corrected, in: app)
        XCTAssertEqual(rows.count, initialCount + 6)
        for (index, id) in ids.enumerated() {
            let row = app.groups[id].firstMatch
            let scroll = app.scrollViews.containing(.any, identifier: id).firstMatch
            // The first row sits flush against the scroll boundary. Reveal the
            // inset thumbnail hit target, not the entire row plus scroll inset.
            let content = row.buttons["Edit layer content"]
            revealEditorControl(content, in: scroll); workspaceActivate(content)
            XCTAssertTrue(row.buttons["Edit layer mask"].exists)
            if corrections[index].2.isEmpty {
                reveal(app.buttons["curve-reset"]); workspaceActivate(app.buttons["curve-reset"])
            } else { edit(corrections[index].2, index == 2 ? "1.2" : "0") }
            _ = changed(corrected); editorHistory("Undo", in: app); expectPixels(corrected, in: app)
        }
        XCTAssertEqual(try Data(contentsOf: photo), original)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "photo-corrections-reopened-and-reedited")
    }
}
#endif
