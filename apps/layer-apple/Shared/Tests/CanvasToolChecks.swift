import XCTest
import CoreGraphics
import ImageIO

extension XCTestCase {
    @MainActor func revealEditorControl(_ element: XCUIElement, in scroll: XCUIElement) {
        XCTAssertTrue(element.waitForExistence(timeout: 5))
        for _ in 0..<10 {
            let viewport = scroll.frame.insetBy(dx: 0, dy: 2), frame = element.frame
            if frame.minY >= viewport.minY && frame.maxY <= viewport.maxY { return }
            let upward = frame.maxY > viewport.maxY
            #if os(macOS)
            scroll.scroll(byDeltaX: 0, deltaY: upward ? -80 : 80)
            #else
            let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.99, dy: upward ? 0.8 : 0.2))
            let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.99, dy: upward ? 0.2 : 0.8))
            start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.2)
            #endif
        }
        XCTFail("Editor control did not become fully visible: \(element.identifier)")
    }
    @MainActor func editorTool(_ label: String, in app: XCUIApplication) {
        workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-toolbar-", label)).firstMatch)
    }

    @MainActor func editorChoice(_ label: String, group: Bool = false, in app: XCUIApplication) {
        let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            group ? "tool-group-" : "tool-subtool-", label)).firstMatch
        workspaceActivate(button)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: button)
        waitForExpectations(timeout: 5)
    }

    @MainActor func editorHistory(_ label: String, in app: XCUIApplication) {
        let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-commands-", label)).firstMatch
        // History is published asynchronously by the Rust owner. In particular,
        // Redo need not be enabled at the instant the preceding Undo click ends.
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: button)
        waitForExpectations(timeout: 10)
        workspaceActivate(button)
    }

    @MainActor func editorPaper(in app: XCUIApplication) -> EditorPaper {
        let source = CGImageSourceCreateWithData(editorScreenshot(in: app).pngRepresentation as CFData, nil)!
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)! as NSDictionary
        let extent = max(properties[kCGImagePropertyPixelWidth] as! Int, properties[kCGImagePropertyPixelHeight] as! Int)
        let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
            kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: extent] as CFDictionary)!
        let width = image.width, height = image.height
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        bytes.withUnsafeMutableBytes { buffer in
            let context = CGContext(data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
        func white(_ x: Int, _ y: Int) -> Bool {
            let i = (y * width + x) * 4
            return bytes[i] >= 253 && bytes[i + 1] >= 253 && bytes[i + 2] >= 253
        }
        var best = (row: 0, start: 0, length: 0)
        for y in stride(from: 0, to: height, by: 2) {
            var start = 0
            for x in 0...width {
                if x < width && white(x, y) { continue }
                if x - start > best.length { best = (y, start, x - start) }
                start = x + 1
            }
        }
        XCTAssertGreaterThan(best.length, width / 10, "The editor must show white paper")
        let column = best.start + best.length / 2
        var top = best.row, bottom = best.row
        while top > 0 && white(column, top - 1) { top -= 1 }
        while bottom < height - 1 && white(column, bottom + 1) { bottom += 1 }
        return EditorPaper(frame: CGRect(x: CGFloat(best.start) / CGFloat(width), y: CGFloat(top) / CGFloat(height),
            width: CGFloat(best.length) / CGFloat(width), height: CGFloat(bottom + 1 - top) / CGFloat(height)))
    }

    @MainActor func editorScreenshot(in app: XCUIApplication) -> XCUIScreenshot {
        #if os(macOS)
        app.windows.firstMatch.screenshot()
        #else
        // The canvas fills the editor window. Its native bounds remain valid
        // after Home/activation, when XCTest's main-display capture can be black.
        app.descendants(matching: .any)["canvas"].firstMatch.screenshot()
        #endif
    }

    @MainActor func editorPixels(in app: XCUIApplication, at point: CGPoint = CGPoint(x: 0.5, y: 0.6), size: Int = 8) -> Data {
        editorPixelSamples(in: app, at: [point], size: size)[0]
    }

    @MainActor func editorPixelSamples(in app: XCUIApplication, at points: [CGPoint], size: Int) -> [Data] {
        // Read all locations from one frame, avoiding repeated native captures.
        let screenshot = editorScreenshot(in: app)
        // Normalize PNG orientation at full resolution before locating samples.
        // A landscape simulator capture can store portrait pixels plus EXIF.
        let source = CGImageSourceCreateWithData(screenshot.pngRepresentation as CFData, nil)!
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)! as NSDictionary
        let extent = max(properties[kCGImagePropertyPixelWidth] as! Int, properties[kCGImagePropertyPixelHeight] as! Int)
        let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
            kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: extent] as CFDictionary)!
        return points.map { point in
            let crop = image.cropping(to: CGRect(x: CGFloat(image.width) * point.x, y: CGFloat(image.height) * point.y,
                width: CGFloat(size), height: CGFloat(size)))!
            var data = Data(count: size * size * 4)
            data.withUnsafeMutableBytes { bytes in
                let context = CGContext(data: bytes.baseAddress, width: size, height: size, bitsPerComponent: 8,
                    bytesPerRow: size * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                context.draw(crop, in: CGRect(x: 0, y: 0, width: size, height: size))
            }
            return data
        }
    }

    @MainActor func attachEditor(in app: XCUIApplication, name: String) {
        let capture = XCTAttachment(screenshot: editorScreenshot(in: app))
        capture.name = name; capture.lifetime = .keepAlways; add(capture)
    }

    @MainActor private func selectPaintPreset(_ tool: String, group: String, preset: String, in app: XCUIApplication) {
        for (prefix, label) in [("toolbar-tile-toolbar-", tool), ("tool-group-", group), ("brush-", preset)] {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", prefix, label)).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout: 5))
            if !button.isSelected {
                let scroll = app.scrollViews.containing(.button, identifier: button.identifier).firstMatch
                if scroll.exists { revealEditorControl(button, in: scroll) }
                workspaceActivate(button)
                expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: button)
                waitForExpectations(timeout: 5)
            }
        }
    }

    @MainActor private func editPaintSize(_ size: String, in app: XCUIApplication) {
        let value = app.buttons["number-value-tool-size"]
        XCTAssertTrue(value.waitForExistence(timeout: 5))
        revealEditorControl(value, in: app.scrollViews.containing(.button, identifier: value.identifier).firstMatch)
        workspaceActivate(value)
        let entry = app.textFields["number-entry-tool-size"]
        XCTAssertTrue(entry.waitForExistence(timeout: 5)); entry.typeText(size + "\n")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", size), evaluatedWith: value)
        waitForExpectations(timeout: 5)
    }

    @MainActor func checkPaintingBrushes(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        let families: [(String, String, [String])] = [
            ("Pen", "Pen", ["G-Pen", "Rough G-Pen", "Calligraphy Pen", "Antique Pen", "Realistic Pen", "Wet Ink", "Blotty Ink", "Realistic Brushed Ink"]),
            ("Pen", "Marker", ["Marker"]),
            ("Pencil", "Pencil", ["Pencil", "Pointy Pencil", "Shading Pencil"]),
            ("Pencil", "Pastel", ["Chalk", "Pastel Block", "Charcoal"]),
            ("Paint Brush", "Paint", ["Paintbrush", "Textured Flat", "Dry Scumble", "Transparent Glaze", "Opaque Gouache", "Multiply Glaze"]),
            ("Paint Brush", "Watercolor", ["Watercolor Wash", "Wet Watercolor"]),
            ("Paint Brush", "Oil paint", ["Loaded Oil", "Palette Knife", "Wet Round"]),
            ("Airbrush", "Airbrush", ["Airbrush"]), ("Airbrush", "Spray", ["Spray"]),
            ("Decoration", "Texture", ["Dual Texture"]), ("Eraser", "Eraser", ["Eraser"]),
        ]
        #if os(macOS)
        func pixels() -> Data { editorPixels(in: app, at: CGPoint(x: 0.5, y: 0.52), size: 96) }
        let paper = pixels()
        func expectPixels(_ value: Data, _ message: String) {
            let match = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in pixels() == value }, object: app)
            if XCTWaiter.wait(for: [match], timeout: 10) != .completed {
                attachEditor(in: app, name: "unexpected-pixels")
                XCTFail(message)
            }
        }
        func settledPixels() -> Data {
            var previous = pixels(), unchanged = 0
            for _ in 0..<40 {
                Thread.sleep(forTimeInterval: 0.25)
                let next = pixels()
                unchanged = next == previous ? unchanged + 1 : 0
                if unchanged == 4 { return next }
                previous = next
            }
            XCTFail("The finished stroke must stop changing"); return previous
        }
        #endif
        for (tool, group, presets) in families {
            for preset in presets {
                selectPaintPreset(tool, group: group, preset: preset, in: app)
                XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "brush-")).count, presets.count)
                editPaintSize("120", in: app)
                XCTAssertTrue(app.buttons["number-value-tool-opacity"].exists)
                #if os(macOS)
                if tool == "Eraser" {
                    editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
                    editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
                    expectation(for: NSPredicate { _, _ in
                        let sample = pixels(); return Int(sample[2]) > Int(sample[0]) + 50
                    }, evaluatedWith: app)
                    waitForExpectations(timeout: 10)
                } else { expectPixels(paper, "\(preset) starts on blank paper") }
                let before = pixels()
                viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.44, dy: 0.55)).click(forDuration: 0.05,
                    thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.62, dy: 0.55)))
                editorDocumentTitle(in: app).hover()
                expectation(for: NSPredicate { _, _ in
                    let sample = pixels()
                    guard sample != before else { return false }
                    for i in stride(from: 0, to: sample.count, by: 4) {
                        if tool == "Eraser" {
                            if sample[i] > 250 && sample[i + 1] > 250 && sample[i + 2] > 250 { return true }
                        } else if Int(sample[i + 2]) > Int(sample[i]) + 10 { return true }
                    }
                    return false
                }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                let painted = settledPixels()
                attachEditor(in: app, name: "paint-" + preset)
                for (command, expected) in [("Undo", before), ("Redo", painted), ("Undo", before)] {
                    editorHistory(command, in: app); expectPixels(expected, "\(preset) \(command) restores the displayed stroke")
                }
                if tool == "Eraser" { editorHistory("Undo", in: app); expectPixels(paper, "Undo restores the paper") }
                #endif
                XCTAssertEqual(viewport.frame, originalFrame)
                XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            }
            #if os(iOS)
            attachEditor(in: app, name: "paint-controls-" + group)
            #endif
        }
    }

    @MainActor func checkBlendAndLiquify(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        #if os(macOS)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        workspaceActivate(app.buttons["color-swap"])
        editorTool("Figure", in: app); editorChoice("Rectangle", group: true, in: app); editorChoice("Fill", in: app)
        viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.38, dy: 0.43)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.53, dy: 0.67)))
        editorDocumentTitle(in: app).hover()
        expectation(for: NSPredicate { _, _ in
            let samples = self.editorPixelSamples(in: app, at: [CGPoint(x: 0.48, y: 0.55), CGPoint(x: 0.58, y: 0.55)], size: 8)
            return Int(samples[0][0]) > Int(samples[0][2]) + 80 && Int(samples[1][2]) > Int(samples[1][0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "blend-two-color-fixture")
        func pixels() -> Data { editorPixels(in: app, at: CGPoint(x: 0.5, y: 0.5), size: 128) }
        let baseline = pixels()
        let outside = [CGPoint(x: 0.4, y: 0.45), CGPoint(x: 0.65, y: 0.68)]
        let untouched = editorPixelSamples(in: app, at: outside, size: 8)
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        #endif
        for (tool, presets) in [("Blend", ["Natural Blender", "Smudge"]), ("Liquify", ["Liquify Push", "Liquify Twirl"])] {
            for preset in presets {
                selectPaintPreset(tool, group: tool, preset: preset, in: app)
                XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "brush-")).count, presets.count)
                editPaintSize("360", in: app)
                XCTAssertEqual(app.buttons["number-value-tool-flow"].exists, tool == "Blend")
                XCTAssertEqual(app.buttons["number-value-tool-strength"].exists, tool == "Liquify")
                #if os(macOS)
                expectPixels(baseline)
                viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.48, dy: 0.55)).click(forDuration: 0.05,
                    thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.55)))
                editorDocumentTitle(in: app).hover()
                expectation(for: NSPredicate { _, _ in pixels() != baseline }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                let changed = pixels()
                XCTAssertEqual(editorPixelSamples(in: app, at: outside, size: 8), untouched,
                    "A local stroke must preserve artwork outside its footprint")
                attachEditor(in: app, name: "paint-" + preset)
                for (command, expected) in [("Undo", baseline), ("Redo", changed), ("Undo", baseline)] {
                    editorHistory(command, in: app); expectPixels(expected)
                }
                #else
                attachEditor(in: app, name: "paint-controls-" + preset)
                #endif
                XCTAssertEqual(viewport.frame, originalFrame)
                XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            }
        }
    }

    @MainActor func checkFiguresAndGradients(in app: XCUIApplication) {
        // Choose contrasting foreground/background colors, without creating artwork.
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        func choose(_ label: String, group: Bool) {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                group ? "tool-group-" : "tool-subtool-", label)).firstMatch
            workspaceActivate(button)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: button)
            waitForExpectations(timeout: 5)
        }
        func tool(_ label: String) {
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-toolbar-", label)).firstMatch)
        }
        #if os(macOS)
        let viewport = workspaceViewport(in: app), paper = editorPaper(in: app)
        func drag(_ a: CGPoint, _ b: CGPoint) {
            viewport.coordinate(withNormalizedOffset: CGVector(dx: a.x, dy: a.y)).click(forDuration: 0.05,
                thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: b.x, dy: b.y)))
            editorDocumentTitle(in: app).hover()
        }
        func samples(_ points: [CGPoint]) -> [Data] { editorPixelSamples(in: app, at: points, size: 4) }
        func waitPixels(_ expected: [Data], _ points: [CGPoint]) {
            expectation(for: NSPredicate { _, _ in samples(points) == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func history(_ points: [CGPoint], blank: [Data], name: String) {
            let painted = samples(points)
            XCTAssertNotEqual(painted, blank, "\(name) must render artwork")
            attachEditor(in: app, name: name)
            for (command, expected) in [("Undo", blank), ("Redo", painted), ("Undo", blank)] {
                workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                    "toolbar-tile-commands-", command)).firstMatch)
                waitPixels(expected, points)
            }
        }
        func hasColor(_ sample: Data, red: Bool) -> Bool {
            let pixels = stride(from: 0, to: sample.count, by: 4)
            let colored = pixels.filter { i in red ? Int(sample[i]) > Int(sample[i + 2]) + 50 : Int(sample[i + 2]) > Int(sample[i]) + 50 }
            return colored.count >= sample.count / 8
        }
        #endif
        tool("Figure")
        for shape in ["Line", "Rectangle", "Ellipse"] {
            choose(shape, group: true)
            let paints = shape == "Line" ? ["Outline"] : ["Outline", "Fill", "Outline + fill"]
            for paint in paints {
                choose(paint, group: false)
                XCTAssertEqual(app.buttons["number-value-tool-size"].exists, paint != "Fill")
                XCTAssertTrue(app.buttons["number-value-tool-opacity"].exists)
                #if os(macOS)
                let center = paper.point(0.425, 0.5)
                let edge = shape == "Line" ? center : paper.point(0.425, 0.2)
                let points = [edge, center, paper.point(0.85, 0.5)]
                let blank = samples(points)
                drag(paper.point(0.25, 0.2), paper.point(0.6, 0.8))
                expectation(for: NSPredicate { _, _ in hasColor(samples(points)[0], red: false) }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                let painted = samples(points)
                if shape != "Line" && paint == "Outline" { XCTAssertEqual(painted[1], blank[1]) }
                else { XCTAssertTrue(hasColor(painted[1], red: paint == "Outline + fill"), "\(shape) \(paint) center color") }
                XCTAssertEqual(painted[2], blank[2], "A figure must leave outside pixels unchanged")
                history(points, blank: blank, name: "figure-\(shape)-\(paint)")
                #endif
            }
        }
        attachEditor(in: app, name: "figure-controls")
        tool("Gradient")
        for radial in [false, true] {
            for transparent in [false, true] {
                let name = "\(radial ? "Radial" : "Linear"): color to \(transparent ? "clear" : "color")"
                choose(name, group: false)
                XCTAssertTrue(app.buttons["number-value-tool-opacity"].exists)
                #if os(macOS)
                let points = [paper.point(0.27, 0.5), paper.point(0.425, 0.5), paper.point(0.85, 0.5)]
                let blank = samples(points)
                drag(paper.point(0.25, 0.5), paper.point(0.6, 0.5))
                expectation(for: NSPredicate { _, _ in hasColor(samples(points)[0], red: false) }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                let painted = samples(points)
                XCTAssertNotEqual(painted[0], painted[1], "The gradient must vary across the canvas")
                if transparent { XCTAssertEqual(painted[2], blank[2]) }
                else { XCTAssertTrue(hasColor(painted[2], red: true), "Color-to-color must reach the background color") }
                history(points, blank: blank, name: "gradient-\(radial)-\(transparent)")
                #endif
            }
        }
        attachEditor(in: app, name: "gradient-controls")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkRulerWorkflow(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-toolbar-", "Ruler")).firstMatch)
        let show = app.buttons["tool-action-show_rulers"]
        let snap = app.buttons["tool-action-snap_rulers"]
        let delete = app.buttons["tool-action-delete_ruler"]
        XCTAssertTrue(show.isSelected && snap.isSelected)
        XCTAssertFalse(delete.isEnabled)
        workspaceActivate(snap); XCTAssertFalse(snap.isSelected)
        workspaceActivate(snap); XCTAssertTrue(snap.isSelected)
        for kind in ["Straight", "Parallel", "Radial"] {
            let choice = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "tool-group-", kind)).firstMatch
            workspaceActivate(choice)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: choice)
            waitForExpectations(timeout: 5)
            #if os(macOS)
            let viewport = workspaceViewport(in: app)
            let start = CGPoint(x: 0.44, y: 0.42), end = CGPoint(x: 0.62, y: 0.68)
            let moved = CGPoint(x: 0.47, y: 0.48)
            // A radial ruler's single center follows the creation drag to its release.
            let handle = kind == "Radial" ? end : start
            func samples() -> [Data] { editorPixelSamples(in: app, at: [handle, moved], size: 16) }
            let blank = samples()
            func waitGuides(_ visible: Bool) {
                expectation(for: NSPredicate { _, _ in (samples() != blank) == visible }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
            }
            func drag(_ a: CGPoint, _ b: CGPoint) {
                viewport.coordinate(withNormalizedOffset: CGVector(dx: a.x, dy: a.y)).click(forDuration: 0.05,
                    thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: b.x, dy: b.y)))
                editorDocumentTitle(in: app).hover()
            }
            drag(start, end)
            waitGuides(true); XCTAssertTrue(delete.isEnabled)
            workspaceActivate(show)
            XCTAssertFalse(show.isSelected); XCTAssertFalse(snap.isEnabled)
            waitGuides(false)
            workspaceActivate(show)
            XCTAssertTrue(show.isSelected); XCTAssertTrue(snap.isEnabled)
            waitGuides(true)
            let original = samples()
            drag(handle, moved)
            expectation(for: NSPredicate { _, _ in samples() != original }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
            attachEditor(in: app, name: "ruler-\(kind)-edited")
            workspaceActivate(delete); waitGuides(false)
            XCTAssertFalse(delete.isEnabled)
            for (command, visible) in [("Undo", true), ("Redo", false)] {
                workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                    "toolbar-tile-commands-", command)).firstMatch)
                waitGuides(visible)
            }
            #endif
        }
        attachEditor(in: app, name: "ruler-controls")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

}

struct EditorPaper {
    let frame: CGRect
    func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
        CGPoint(x: frame.minX + x * frame.width, y: frame.minY + y * frame.height)
    }
    func offset(_ x: CGFloat, _ y: CGFloat) -> CGVector {
        let point = point(x, y)
        return CGVector(dx: point.x, dy: point.y)
    }
}
