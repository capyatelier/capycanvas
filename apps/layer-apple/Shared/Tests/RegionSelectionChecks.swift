import XCTest
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

#if os(macOS)
extension XCTestCase {
    @MainActor func checkNativeRegionRefinement(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Region " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0,0,1,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "File", id: "new_document", label: "New…")
        for id in ["width", "height"] {
            let field = app.textFields["new-document-" + id]
            workspaceActivate(field); field.typeKey("a", modifierFlags: .command); field.typeText("64")
        }
        workspaceActivate(app.buttons["new-document-create"])
        XCTAssertTrue(app.buttons["new-document-create"].waitForNonExistence(timeout: 15))
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        let title = app.staticTexts["document-title"]
        let accept = app.windows.buttons["OKButton"].firstMatch
        func goTo(_ url: URL) {
            app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(url.path + "\n")
        }
        func outline(gap: Bool) throws -> Data {
            let bytes = Data((0..<64).flatMap { y in (0..<64).flatMap { x -> [UInt8] in
                let wall = (16..<48).contains(x) && (16..<48).contains(y)
                    && (!(20..<44).contains(x) || !(20..<44).contains(y))
                    && !(gap && (31..<33).contains(x) && y < 20)
                return wall ? [0, 0, 0, 255] : [255, 255, 255, 255]
            } })
            let provider = try XCTUnwrap(CGDataProvider(data: bytes as CFData))
            let image = try XCTUnwrap(CGImage(width: 64, height: 64, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: 256, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
            let url = root.appendingPathComponent(gap ? "Broken outline.png" : "Closed outline.png")
            let output = try XCTUnwrap(CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil))
            CGImageDestinationAddImage(output, image, nil); XCTAssertTrue(CGImageDestinationFinalize(output))
            workspaceActivate(app.buttons["layer-Import image as layer"])
            XCTAssertTrue(accept.waitForExistence(timeout: 15)); goTo(url); workspaceActivate(accept)
            XCTAssertTrue(accept.waitForNonExistence(timeout: 15))
            XCTAssertTrue(app.staticTexts[url.lastPathComponent].firstMatch.waitForExistence(timeout: 15))
            return try pixels()
        }
        var exportIndex = 0
        func pixels() throws -> Data {
            exportIndex += 1
            let url = root.appendingPathComponent("Result-\(exportIndex).png")
            editorMenu(in: app, menu: "File", id: "export_document", label: "Export…")
            chooseExportDestination(in: app)
            XCTAssertTrue(accept.waitForExistence(timeout: 15)); goTo(root)
            let name = app.textFields["saveAsNameTextField"]
            workspaceActivate(name); name.typeKey("a", modifierFlags: .command); name.typeText(url.lastPathComponent)
            workspaceActivate(accept); XCTAssertTrue(accept.waitForNonExistence(timeout: 15))
            expectation(for: NSPredicate { _, _ in FileManager.default.fileExists(atPath: url.path) }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            let source = try XCTUnwrap(CGImageSourceCreateWithURL(url as CFURL, nil))
            let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(source, 0, nil))
            XCTAssertEqual(image.width, 64); XCTAssertEqual(image.height, 64)
            var bytes = Data(count: 64 * 64 * 4)
            bytes.withUnsafeMutableBytes { buffer in
                let context = CGContext(data: buffer.baseAddress, width: 64, height: 64, bitsPerComponent: 8,
                    bytesPerRow: 256, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                context.draw(image, in: CGRect(x: 0, y: 0, width: 64, height: 64))
            }
            return bytes
        }
        func setting(_ id: String, _ value: Int) {
            let entry = app.textFields["number-entry-tool-" + id]
            if entry.exists {
                revealEditorControl(entry, in: app.scrollViews.containing(.textField, identifier: entry.identifier).firstMatch)
            } else {
                let button = app.buttons["number-value-tool-" + id]
                revealEditorControl(button, in: app.scrollViews.containing(.button, identifier: button.identifier).firstMatch)
                workspaceActivate(button)
            }
            workspaceActivate(entry); entry.typeKey("a", modifierFlags: .command); entry.typeText("\(value)\n")
            XCTAssertFalse(app.staticTexts["number-error-tool-" + id].exists)
        }
        func paint(_ tool: String) throws -> Data {
            viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.55)).click(); title.hover()
            if tool == "Auto select" {
                editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
            }
            return try pixels()
        }
        let closed = try outline(gap: false)
        for tool in ["Fill", "Auto select"] {
            editorTool(tool, in: app); editorChoice("Visible artwork", in: app)
            setting("tolerance", 0); setting("gap_closing", 0)
            for (expansion, smoothing) in [(0, 0), (2, 0), (-2, 0), (0, 100)] {
                setting("expansion", expansion); setting("smoothing", smoothing)
                let painted = try paint(tool)
                var softened = 0
                for i in 0..<(64 * 64) {
                    let x = i % 64, y = i / 64, actual = Array(painted[(i * 4)..<(i * 4 + 4)])
                    if (20 - expansion..<44 + expansion).contains(x) && (20 - expansion..<44 + expansion).contains(y) {
                        if smoothing == 0 { XCTAssertEqual(actual, [0, 0, 255, 255], "\(tool) interior \(x),\(y)") }
                        else {
                            XCTAssertEqual(Array(actual[2...]), [255, 255])
                            if actual[0] > 0 && actual[0] < 255 { softened += 1 }
                        }
                    } else { XCTAssertEqual(Data(actual), closed[(i * 4)..<(i * 4 + 4)], "\(tool) exterior \(x),\(y)") }
                }
                if smoothing != 0 { XCTAssertEqual(softened, 4, "Only the four corners need softened coverage") }
                attachEditor(in: app, name: "native-\(tool)-expansion-\(expansion)-smoothing-\(smoothing)")
                editorHistory("Undo", in: app); XCTAssertEqual(try pixels(), closed)
                // Check exact native history once per tool, including softened pixels.
                if smoothing != 0 {
                    editorHistory("Redo", in: app); XCTAssertEqual(try pixels(), painted)
                    editorHistory("Undo", in: app)
                }
                if tool == "Auto select" { editorHistory("Undo", in: app) }
            }
        }
        let broken = try outline(gap: true)
        editorTool("Fill", in: app); setting("expansion", 0); setting("smoothing", 0)
        for distance in [0, 4] {
            setting("gap_closing", distance)
            let painted = try paint("Fill")
            XCTAssertEqual(Array(painted[(32 * 64 + 32) * 4..<(32 * 64 + 32) * 4 + 4]), [0, 0, 255, 255])
            XCTAssertEqual(Array(painted.prefix(4)), distance == 0 ? [0, 0, 255, 255] : [255, 255, 255, 255])
            attachEditor(in: app, name: "native-gap-closing-\(distance)")
            editorHistory("Undo", in: app); XCTAssertEqual(try pixels(), broken)
        }
        XCTAssertEqual(viewport.frame, originalFrame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
#endif

extension XCTestCase {
    @MainActor func checkRegionSelectionAndFill(in app: XCUIApplication) {
        // Mac artwork cases need contrasting colors. UIKit exercises controls
        // only, so it needs no startup actions while scene ownership settles.
        #if os(macOS)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        #endif
        app.launch(); capturePaintEditor(in: app)
        #if os(macOS)
        let viewport = workspaceViewport(in: app)
        let center = CGPoint(x: 0.48, y: 0.55)
        let points = [center, CGPoint(x: 0.58, y: 0.55), CGPoint(x: 0.66, y: 0.55)]
        func coordinate(_ point: CGPoint) -> XCUICoordinate {
            viewport.coordinate(withNormalizedOffset: CGVector(dx: point.x, dy: point.y))
        }
        func samples() -> [Data] { editorPixelSamples(in: app, at: points, size: 4) }
        func waitPixels(_ expected: [Data]) {
            expectation(for: NSPredicate { _, _ in samples() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func red(_ data: Data) -> Bool { Int(data[0]) > Int(data[2]) + 50 }
        // A separate outline layer makes the three region sources distinguishable.
        editorTool("Figure", in: app)
        editorChoice("Rectangle", group: true, in: app); editorChoice("Outline", in: app)
        coordinate(CGPoint(x: 0.44, y: 0.42)).click(forDuration: 0.05,
            thenDragTo: coordinate(CGPoint(x: 0.62, y: 0.68)))
        app.staticTexts["document-title"].hover()
        workspaceActivate(app.buttons["layer-Use selected layers as references"])
        XCTAssertTrue(app.buttons["layer-Stop using this layer as a reference"].isSelected)
        workspaceActivate(app.buttons["layer-New layer"])
        // Visible artwork includes this unmarked divider; reference-only sampling ignores it.
        editorChoice("Line", group: true, in: app)
        coordinate(CGPoint(x: 0.53, y: 0.42)).click(forDuration: 0.05,
            thenDragTo: coordinate(CGPoint(x: 0.53, y: 0.68)))
        app.staticTexts["document-title"].hover()
        workspaceActivate(app.buttons["layer-New layer"])
        workspaceActivate(app.buttons["color-swap"])
        let blank = samples()
        func fillResult(_ source: String, inverted: Bool = false, name: String) {
            expectation(for: NSPredicate { _, _ in red(samples()[inverted ? 1 : 0]) }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            let painted = samples()
            let coverage = source == "Editing layer" ? [true, true, true]
                : source == "Reference layers" ? [true, true, false] : [true, false, false]
            for index in points.indices {
                if coverage[index] != inverted { XCTAssertTrue(red(painted[index]), "\(name) must paint sample \(index)") }
                else { XCTAssertEqual(painted[index], blank[index], "\(name) must preserve sample \(index)") }
            }
            attachEditor(in: app, name: name)
            for (command, pixels) in [("Undo", blank), ("Redo", painted), ("Undo", blank)] {
                editorHistory(command, in: app); waitPixels(pixels)
            }
        }
        #endif
        for tool in ["Fill", "Auto select"] {
            editorTool(tool, in: app)
            for source in ["Visible artwork", "Editing layer", "Reference layers"] {
                editorChoice(source, in: app)
                for setting in ["tolerance", "smoothing"] {
                    XCTAssertTrue(app.buttons["number-value-tool-" + setting].exists)
                }
                for setting in ["gap_closing", "expansion"] {
                    XCTAssertTrue(app.textFields["number-entry-tool-" + setting].exists)
                }
                XCTAssertEqual(app.buttons["number-value-tool-opacity"].exists, tool == "Fill")
                #if os(macOS)
                coordinate(center).click(); app.staticTexts["document-title"].hover()
                if tool == "Auto select" {
                    let select = app.menuBars.menuBarItems["Select"]
                    workspaceActivate(select)
                    expectation(for: NSPredicate(format: "enabled == YES"),
                        evaluatedWith: select.menuItems["Invert selection"])
                    waitForExpectations(timeout: 10)
                    app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
                    editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
                }
                fillResult(source, name: "region-\(tool)-\(source)")
                if tool == "Auto select" {
                    if source == "Visible artwork" {
                        editorMenu(in: app, menu: "Select", id: "invert_selection", label: "Invert selection")
                        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
                        fillResult(source, inverted: true, name: "region-inverted")
                    }
                    editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
                }
                #endif
            }
            // Exercise the real numeric editor, including restoration of the original value.
            for percent in [20, 10] {
                workspaceActivate(app.buttons["number-value-tool-tolerance"])
                let entry = app.textFields["number-entry-tool-tolerance"]
                XCTAssertTrue(entry.waitForExistence(timeout: 5))
                entry.typeText("\(percent)\n")
                expectation(for: NSPredicate(format: "value == %@", "\(percent).0 %"),
                    evaluatedWith: app.buttons["number-value-tool-tolerance"])
                waitForExpectations(timeout: 5)
            }
            #if os(iOS)
            // Exercise the integer fields and the lower, scrollable smoothing
            // control through UIKit. Switching tools verifies owner publication,
            // rather than merely observing the field's unfinished local draft.
            for (id, values) in [("gap_closing", ["4", "0"]), ("expansion", ["2", "-2", "0"])] {
                for value in values {
                    let entry = app.textFields["number-entry-tool-" + id]
                    revealEditorControl(entry, in: app.scrollViews.containing(.textField, identifier: entry.identifier).firstMatch)
                    workspaceActivate(entry); entry.typeText(value + "\n")
                    editorTool("Brush", in: app); editorTool(tool, in: app)
                    expectation(for: NSPredicate(format: "value == %@", value + " px"), evaluatedWith: entry)
                    waitForExpectations(timeout: 5)
                }
            }
            for percent in [25, 100] {
                let value = app.buttons["number-value-tool-smoothing"]
                revealEditorControl(value, in: app.scrollViews.containing(.button, identifier: value.identifier).firstMatch)
                workspaceActivate(value)
                let entry = app.textFields["number-entry-tool-smoothing"]
                XCTAssertTrue(entry.waitForExistence(timeout: 5)); entry.typeText("\(percent)\n")
                editorTool("Brush", in: app); editorTool(tool, in: app)
                expectation(for: NSPredicate(format: "value == %@", "\(percent).0 %"), evaluatedWith: value)
                waitForExpectations(timeout: 5)
            }
            #endif
            attachEditor(in: app, name: "region-\(tool)-controls")
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkSelectionInversion(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        func pixels() -> Data { editorPixels(in: app) }
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func invert() { editorMenu(in: app, menu: "Select", id: "invert_selection", label: "Invert selection") }
        func fill() { editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection") }
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        fill()
        expectation(for: NSPredicate { _, _ in let p = pixels(); return Int(p[2]) > Int(p[0]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let blue = pixels()
        workspaceActivate(app.buttons["color-swap"])
        invert(); fill(); expectPixels(blue)
        attachEditor(in: app, name: "selection-inverted-empty")
        invert(); fill()
        expectation(for: NSPredicate { _, _ in let p = pixels(); return Int(p[0]) > Int(p[2]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let red = pixels()
        attachEditor(in: app, name: "selection-inverted-full")
        editorHistory("Undo", in: app); expectPixels(blue)
        editorHistory("Redo", in: app); expectPixels(red)
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
