import XCTest
import CoreGraphics
import ImageIO

extension XCTestCase {
    @MainActor func editorText(_ element: XCUIElement) -> String {
        element.label.isEmpty ? element.value as? String ?? "" : element.label
    }

    @MainActor func revealEditorControl(_ element: XCUIElement, in scroll: XCUIElement) {
        XCTAssertTrue(element.waitForExistence(timeout: 5))
        guard let requested = try? element.snapshot(), let original = try? scroll.snapshot() else {
            return XCTFail("The scroll must expose its requested control before native input")
        }
        let identifier = requested.identifier, type = requested.elementType
        func identity(_ node: XCUIElementSnapshot) -> String {
            if !identifier.isEmpty { return node.identifier }
            return node.label.isEmpty ? node.value as? String ?? "" : node.label
        }
        func descendants(_ node: XCUIElementSnapshot) -> [XCUIElementSnapshot] {
            node.children.flatMap { [$0] + descendants($0) }
        }
        let key = identity(requested)
        let order = descendants(original).filter { $0.elementType == type }.map(identity).filter { !$0.isEmpty }
        guard !key.isEmpty, Set(order).count == order.count, let index = order.firstIndex(of: key) else {
            return XCTFail("The scroll must retain unique identities for its requested control and siblings: \(identifier)")
        }
        for _ in 0..<30 {
            guard let current = try? scroll.snapshot() else { return XCTFail("The native scroll viewport must remain available") }
            let nodes = descendants(current), viewport = current.frame
            let siblings = nodes.filter { $0.elementType == type }
            let frame = siblings.first { identity($0) == key }?.frame ?? .zero
            if frame.width > 0 && frame.height > 0 && viewport.contains(frame) {
                guard element.exists else { return XCTFail("The fully visible requested control must resolve: \(identifier)") }
                return
            }
            let upward: Bool
            if frame.width > 0 && frame.height > 0 { upward = frame.maxY > viewport.maxY }
            else {
                let visible = siblings.filter {
                    let bounds = $0.frame
                    return !identity($0).isEmpty && bounds.width > 0 && bounds.height > 0 && viewport.contains(bounds)
                }.map(identity)
                guard visible.allSatisfy({ order.contains($0) }) else {
                    return XCTFail("Visible controls must retain their original sibling order: \(identifier)")
                }
                let ranks = visible.compactMap { order.firstIndex(of: $0) }
                let before = ranks.contains { $0 < index }, after = ranks.contains { $0 > index }
                guard before != after else {
                    return XCTFail("The hidden control must lie outside its visible siblings: \(identifier)")
                }
                upward = before
            }
            #if os(macOS)
            let window = XCUIApplication().windows.firstMatch, windowBounds = window.frame
            guard let target = nodes.first(where: {
                let bounds = $0.frame
                return $0.elementType == .button && !$0.identifier.isEmpty && bounds.width > 0 && bounds.height > 0
                    && bounds.midX.isFinite && bounds.midY.isFinite
                    && viewport.contains(bounds) && windowBounds.contains(bounds)
            }) else {
                return XCTFail("The scroll viewport must contain a visible wheel target inside its editor window: \(scroll.identifier)")
            }
            let bounds = target.frame, targetID = target.identifier
            let wheel = nodes.first { $0.elementType == .staticText && $0.frame.width > 0 && $0.frame.height > 0
                && viewport.contains($0.frame) && windowBounds.contains($0.frame) }?.frame ?? viewport
            let point = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: wheel.midX - windowBounds.minX, dy: wheel.midY - windowBounds.minY))
            point.hover()
            point.scroll(byDeltaX: 0, deltaY: upward ? -80 : 80)
            expectation(for: NSPredicate { _, _ in
                guard let next = try? scroll.snapshot() else { return false }
                let nextNodes = descendants(next)
                let movedRequest = nextNodes.first { $0.elementType == type && identity($0) == key }?.frame
                let movedTarget = nextNodes.first { $0.elementType == .button && $0.identifier == targetID }?.frame
                return movedRequest.map { $0.width > 0 && $0.height > 0 && $0 != frame } == true
                    || movedTarget.map { $0.width > 0 && $0.height > 0 && $0 != bounds } == true
            }, evaluatedWith: scroll)
            waitForExpectations(timeout: 10)
            #else
            let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: upward ? 0.8 : 0.2))
            let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: upward ? 0.2 : 0.8))
            start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.2)
            #endif
        }
        XCTFail("Editor control did not become fully visible: \(identifier)")
    }
    @MainActor func revealEditorControl(_ element: XCUIElement, identifier: String = "", label: String = "", type: XCUIElement.ElementType, in scroll: XCUIElement) {
        let identity = identifier.isEmpty ? label : identifier
        guard !identity.isEmpty, scroll.waitForExistence(timeout: 5) else {
            return XCTFail("A lazy control needs its stable identity and native scroll")
        }
        func descendants(_ node: XCUIElementSnapshot) -> [XCUIElementSnapshot] {
            node.children.flatMap { [$0] + descendants($0) }
        }
        func key(_ node: XCUIElementSnapshot) -> String { "\(node.elementType.rawValue):\(node.identifier)" }
        func positive(_ frame: CGRect) -> Bool {
            frame.minX.isFinite && frame.minY.isFinite && frame.width.isFinite && frame.height.isFinite
                && frame.width > 0 && frame.height > 0
        }
        func attach(_ text: String) {
            let proof = XCTAttachment(string: text)
            proof.name = "native-lazy-reveal-" + identity
            proof.lifetime = .keepAlways
            add(proof)
        }
        let window = XCUIApplication().windows.firstMatch, windowBounds = window.frame
        var towardEnd = true, reversed = false
        for step in 0..<24 {
            guard let current = try? scroll.snapshot() else { return XCTFail("The native scroll must remain mounted") }
            let viewport = current.frame.intersection(windowBounds), nodes = descendants(current)
            guard positive(viewport) else { return XCTFail("The native scroll needs a finite viewport inside its window") }
            let matches = nodes.filter {
                (identifier.isEmpty ? $0.label == label : $0.identifier == identifier) && (type == .any || $0.elementType == type)
            }
            guard matches.count <= 1 else { return XCTFail("The lazy control must have one semantic identity: \(identity)") }
            if let anchor = matches.first {
                let frame = identifier.isEmpty ? (try? element.snapshot())?.frame ?? .zero : anchor.frame
                guard !positive(frame) || frame.width <= viewport.width && frame.height <= viewport.height else {
                    return XCTFail("The requested control must fit its native viewport: \(identity)")
                }
                if positive(frame), viewport.contains(frame) {
                    guard element.exists else { return XCTFail("The fully contained lazy control must resolve: \(identity)") }
                    attach("Step \(step): fully contained \(identity), type \(type.rawValue), frame \(frame), viewport \(viewport), window \(windowBounds)")
                    return
                }
                let bounds = positive(frame) ? frame : anchor.frame
                if positive(bounds) { towardEnd = bounds.maxY > viewport.maxY }
            }
            let visible = nodes.filter { !$0.identifier.isEmpty && positive($0.frame) && viewport.contains($0.frame) }
            let groups = Dictionary(grouping: visible, by: key)
            let anchors = groups.filter { $0.value.count == 1 }.mapValues { $0[0].frame }
            guard !anchors.isEmpty else { return XCTFail("Native lazy scrolling needs visible identified controls to measure progress") }
            #if os(macOS)
            let point = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (identifier.isEmpty ? viewport.midX : viewport.minX + 2) - windowBounds.minX, dy: viewport.midY - windowBounds.minY))
            point.hover()
            point.scroll(byDeltaX: 0, deltaY: towardEnd ? -80 : 80)
            #else
            let x = viewport.minX - windowBounds.minX + viewport.width * 0.05
            let start = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: x,
                dy: viewport.minY - windowBounds.minY + viewport.height * (towardEnd ? 0.8 : 0.2)))
            let end = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: x,
                dy: viewport.minY - windowBounds.minY + viewport.height * (towardEnd ? 0.2 : 0.8)))
            start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.2)
            #endif
            var moved: [String] = []
            let progress = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                guard let next = try? scroll.snapshot() else { return false }
                let nextGroups = Dictionary(grouping: descendants(next), by: key)
                moved = nextGroups.values.compactMap { group in
                    guard group.count == 1, let node = group.first else { return nil }
                    guard let before = anchors[key(node)], positive(node.frame), abs(node.frame.minY - before.minY) > 1 else { return nil }
                    return "\(key(node)): \(before) → \(node.frame)"
                }
                return !moved.isEmpty
            }, object: scroll)
            let changed = XCTWaiter.wait(for: [progress], timeout: 2) == .completed
            attach("Step \(step): toward end \(towardEnd), viewport \(viewport), measured progress \(changed)\n" + moved.joined(separator: "\n"))
            if !changed {
                guard !reversed else { return XCTFail("The lazy control was not found across both measured scroll boundaries: \(identity)") }
                towardEnd.toggle(); reversed = true
            }
        }
        XCTFail("The lazy control did not become fully contained within the bounded native search: \(identity)")
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
        let buttons = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-", label))
        let button = buttons.firstMatch
        // History is published asynchronously by the Rust owner. In particular,
        // Redo need not be enabled at the instant the preceding Undo click ends.
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: button)
        waitForExpectations(timeout: 10)
        XCTAssertEqual(buttons.count, 1, "The active workspace needs one \(label) history control")
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

    @MainActor private func settledEditorPixels(in app: XCUIApplication, at point: CGPoint, size: Int) -> Data {
        var previous = editorPixels(in: app, at: point, size: size), unchanged = 0
        for _ in 0..<40 {
            Thread.sleep(forTimeInterval: 0.25)
            let next = editorPixels(in: app, at: point, size: size)
            unchanged = next == previous ? unchanged + 1 : 0
            if unchanged == 4 { return next }
            previous = next
        }
        XCTFail("The finished stroke must stop changing"); return previous
    }

    @MainActor func expectPixels(_ expected: Data, in app: XCUIApplication, at point: CGPoint = CGPoint(x: 0.5, y: 0.6)) {
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: point) == expected }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
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

    @MainActor private func selectPaintChoice(_ prefix: String, _ label: String, in app: XCUIApplication) {
        let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", prefix, label)).firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 5), "The current tool controls must offer \(label)")
        if !button.isSelected {
            let scroll = app.scrollViews.containing(.button, identifier: button.identifier).firstMatch
            if scroll.exists { revealEditorControl(button, in: scroll) }
            workspaceActivate(button)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: button)
            waitForExpectations(timeout: 5)
        }
    }
    @MainActor func selectPaintGroup(_ tool: String, group: String, in app: XCUIApplication) {
        let category = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "tool-subtool-", group)).firstMatch
        let parent = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "tool-group-", group)).firstMatch
        if !parent.exists && !category.exists { selectPaintChoice("toolbar-tile-toolbar-", tool, in: app) }
        if parent.waitForExistence(timeout: 1) {
            selectPaintChoice("tool-group-", group, in: app)
        } else if category.exists {
            selectPaintChoice("tool-subtool-", group, in: app)
        } else {
            XCTAssertEqual(group, tool, "A tool with one category omits its redundant category button")
        }
    }
    @MainActor func selectPaintPreset(_ tool: String, group: String, preset: String, in app: XCUIApplication) {
        selectPaintGroup(tool, group: group, in: app)
        selectPaintChoice("brush-", preset, in: app)
    }

    @MainActor private func editPaintSetting(_ id: String, text: String, in app: XCUIApplication) -> XCUIElement {
        let value = app.buttons["number-value-tool-" + id]
        XCTAssertTrue(value.waitForExistence(timeout: 5))
        revealEditorControl(value, in: app.scrollViews.containing(.button, identifier: value.identifier).firstMatch)
        workspaceActivate(value)
        let entry = app.textFields["number-entry-tool-" + id]
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        replaceEditorText(entry, with: text, in: app)
        entry.typeText("\n")
        return value
    }

    @MainActor func editPaintSize(_ size: String, in app: XCUIApplication) {
        let value = editPaintSetting("size", text: size, in: app)
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", size), evaluatedWith: value)
        waitForExpectations(timeout: 5)
    }

    @MainActor private func resetPaintSetting(_ id: String, in app: XCUIApplication) {
        let value = app.buttons["number-value-tool-" + id]
        let scroll = app.scrollViews.containing(.button, identifier: value.identifier).firstMatch
        revealEditorControl(value, in: scroll)
        let labels = scroll.staticTexts.allElementsBoundByIndex.filter { editorText($0) == value.label }
        guard let label = labels.min(by: { abs($0.frame.midY - value.frame.midY) < abs($1.frame.midY - value.frame.midY) }) else {
            return XCTFail("The numeric control must expose its own label")
        }
        XCTAssertLessThan(abs(label.frame.midY - value.frame.midY), 36)
        #if os(macOS)
        label.doubleClick()
        #else
        let window = app.windows.firstMatch, frame = label.frame
        guard frame.width > 0, frame.height > 0, scroll.frame.contains(frame), window.frame.contains(frame) else {
            return XCTFail("The numeric caption must be visible inside its editor window")
        }
        window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: frame.midX - window.frame.minX, dy: frame.midY - window.frame.minY)).doubleTap()
        #endif
    }

    @MainActor private func expectPaintGroup(_ title: String, setting id: String, in app: XCUIApplication) {
        let value = app.buttons["number-value-tool-" + id]
        let scroll = app.scrollViews.containing(.button, identifier: value.identifier).firstMatch
        revealEditorControl(value, in: scroll)
        guard let snapshot = try? scroll.snapshot() else { return XCTFail("The tool settings scroll must remain available") }
        func descendants(_ node: XCUIElementSnapshot) -> [XCUIElementSnapshot] {
            node.children.flatMap { [$0] + descendants($0) }
        }
        func text(_ node: XCUIElementSnapshot) -> String {
            node.label.isEmpty ? node.value as? String ?? "" : node.label
        }
        let nodes = descendants(snapshot)
        guard let heading = nodes.first(where: { $0.elementType == .staticText && text($0) == title }),
              let readout = nodes.first(where: { $0.elementType == .button && $0.identifier == value.identifier }) else {
            return XCTFail("The setting must expose its section heading and value: \(title)")
        }
        XCTAssertTrue(snapshot.frame.intersects(heading.frame))
        XCTAssertTrue(readout.frame.width > 0 && readout.frame.height > 0 && snapshot.frame.contains(readout.frame))
        XCTAssertLessThan(heading.frame.maxY, readout.frame.minY)
        let sections = nodes.filter {
            $0.elementType == .staticText && ["Tip", "Material", "Texture", "Blending", "Smoothing"].contains(text($0))
        }
        XCTAssertFalse(sections.contains { $0.frame.minY > heading.frame.maxY && $0.frame.maxY < readout.frame.minY }, "A setting must remain under its own heading")
    }

    @MainActor func checkPenBrushOrder(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch(); capturePaintEditor(in: app)
        selectPaintPreset("Pen", group: "Pen", preset: "G-Pen", in: app)
        let presets = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "brush-"))
        let order = presets.allElementsBoundByIndex.map(\.label)
        XCTAssertEqual(Array(order.prefix(3)), ["G-Pen", "Monoline", "Rough G-Pen"])
        selectPaintPreset("Pen", group: "Pen", preset: "Blotty Ink", in: app)
        XCTAssertTrue(presets.matching(NSPredicate(format: "label == %@", "Blotty Ink")).firstMatch.isSelected)
        XCTAssertEqual(presets.allElementsBoundByIndex.map(\.label), order)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "pen-brush-order-" + theme)
    }

    #if os(macOS)
    @MainActor func checkDemandShaderFirstUse(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        let viewport = workspaceViewport(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        workspaceActivate(app.buttons["color-swap"])
        editorTool("Line", in: app); editorChoice("Rectangle", group: true, in: app); editorChoice("Fill", in: app)
        viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.38, dy: 0.43)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.53, dy: 0.67)))
        editorDocumentTitle(in: app).hover()
        expectation(for: NSPredicate { _, _ in
            let samples = self.editorPixelSamples(in: app, at: [CGPoint(x: 0.48, y: 0.55), CGPoint(x: 0.58, y: 0.55)], size: 8)
            return Int(samples[0][0]) > Int(samples[0][2]) + 80 && Int(samples[1][2]) > Int(samples[1][0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        let sample = CGPoint(x: 0.5, y: 0.5)
        func pixels() -> Data { editorPixels(in: app, at: sample, size: 128) }
        func expect(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
        }
        let baseline = pixels()
        for (tool, group, preset) in [("Pen", "Pen", "G-Pen"), ("Paint Brush", "Paint", "Dry Scumble"),
            ("Paint Brush", "Watercolor", "Watercolor Wash"), ("Blend", "Paint mixing", "Natural Blender"),
            ("Liquify", "Liquify", "Liquify Push")] {
            selectPaintPreset(tool, group: group, preset: preset, in: app)
            for replay in 0...1 {
                expect(baseline)
                viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.48, dy: 0.55)).click(forDuration: 0.05,
                    thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.55)))
                editorDocumentTitle(in: app).hover()
                expectation(for: NSPredicate { _, _ in pixels() != baseline }, evaluatedWith: app)
                waitForExpectations(timeout: 30)
                let painted = pixels()
                attachEditor(in: app, name: "demand-first-use-\(preset)-\(replay)-\(theme)")
                for (command, expected) in [("Undo", baseline), ("Redo", painted), ("Undo", baseline)] {
                    editorHistory(command, in: app); expect(expected)
                }
                XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            }
        }
    }
    #endif

    @MainActor func checkPaintingBrushes(in app: XCUIApplication, theme: String = "light", dryOnly: Bool = false) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        let families: [(String, String, [String])] = [
            ("Pen", "Pen", ["G-Pen", "Monoline", "Rough G-Pen", "Blotty Ink", "Calligraphy Pen", "Antique Pen", "Realistic Pen", "Wet Ink", "Realistic Brushed Ink", "Marker"]),
            ("Pencil", "Pencil", ["Pencil", "Pointy Pencil", "Shading Pencil"]),
            ("Pencil", "Pastel", ["Chalk", "Pastel Block", "Charcoal", "Crayon"]),
            ("Paint Brush", "Paint", ["Paintbrush", "Bristle Paintbrush", "Textured Flat", "Dry Scumble", "Transparent Glaze", "Opaque Gouache", "Multiply Glaze", "Dual Texture"]),
            ("Paint Brush", "Watercolor", ["Watercolor Wash", "Wet Watercolor"]),
            ("Paint Brush", "Oil paint", ["Loaded Oil", "Palette Knife", "Wet Round", "Mixing Brush"]),
            ("Airbrush", "Airbrush", ["Airbrush", "Spray"]), ("Eraser", "Eraser", ["Eraser"]),
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
            settledEditorPixels(in: app, at: CGPoint(x: 0.5, y: 0.52), size: 96)
        }
        #endif
        for (tool, group, presets) in families where !dryOnly || tool == "Pencil" {
            for preset in presets {
                if presets.count == 1 { selectPaintGroup(tool, group: group, in: app) }
                else { selectPaintPreset(tool, group: group, preset: preset, in: app) }
                XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "brush-")).count,
                    presets.count == 1 ? 0 : presets.count)
                editPaintSize("120", in: app)
                XCTAssertTrue(app.buttons["number-value-tool-opacity"].exists)
                if tool == "Pencil" {
                    let history = ["Undo", "Redo"].map { app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", $0)).firstMatch }
                    let historyBefore = history.map(\.isEnabled), paperBefore = editorPixels(in: app)
                    let primary = ["size", "flow", "grain_depth", "material_hardness"].map { app.buttons["number-value-tool-" + $0] }
                    XCTAssertTrue(primary.allSatisfy { $0.exists && $0.frame.height > 0 })
                    let y = primary.map { $0.frame.midY }
                    XCTAssertTrue(zip(y, y.dropFirst()).allSatisfy { $0.0 < $0.1 }, "Size, Strength, Texture strength and Material hardness stay primary")
                    let scroll = app.scrollViews.containing(.button, identifier: primary[0].identifier).firstMatch
                    let captions = scroll.staticTexts.allElementsBoundByIndex.map { editorText($0) }
                    for heading in ["Material", "Texture"] { XCTAssertFalse(captions.contains(heading), "Promoted dry controls have no duplicate section heading") }
                    expectPaintGroup("Blending", setting: "opacity", in: app)
                    let opacity = app.buttons["number-value-tool-opacity"].value as? String
                    for id in ["material_hardness", "grain_depth"] {
                        let value = app.buttons["number-value-tool-" + id], other = app.buttons["number-value-tool-" + (id == "grain_depth" ? "material_hardness" : "grain_depth")]
                        guard let original = value.value as? String, let unchanged = other.value as? String else {
                            return XCTFail("Dry controls must expose their accepted values")
                        }
                        for text in id == "grain_depth" ? ["37", "200"] : ["37"] {
                            _ = editPaintSetting(id, text: text, in: app)
                            expectation(for: NSPredicate(format: "value == %@", text + " %"), evaluatedWith: value)
                            waitForExpectations(timeout: 10)
                            XCTAssertEqual(other.value as? String, unchanged); XCTAssertEqual(app.buttons["number-value-tool-opacity"].value as? String, opacity)
                        }
                        resetPaintSetting(id, in: app)
                        expectation(for: NSPredicate(format: "value == %@", original), evaluatedWith: value)
                        waitForExpectations(timeout: 10)
                    }
                    XCTAssertEqual(history.map(\.isEnabled), historyBefore)
                    XCTAssertEqual(editorPixels(in: app), paperBefore, "Material controls preserve artwork")
                    attachEditor(in: app, name: "dry-material-controls-\(preset)-\(theme)")
                }
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
                attachEditor(in: app, name: "paint-\(preset)-\(theme)")
                for (command, expected) in [("Undo", before), ("Redo", painted), ("Undo", before)] {
                    editorHistory(command, in: app); expectPixels(expected, "\(preset) \(command) restores the displayed stroke")
                }
                if tool == "Eraser" { editorHistory("Undo", in: app); expectPixels(paper, "Undo restores the paper") }
                #endif
                XCTAssertEqual(viewport.frame, originalFrame)
                XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            }
            #if os(iOS)
            attachEditor(in: app, name: "paint-controls-\(group)-\(theme)")
            #endif
        }
    }

    @MainActor func checkBlendAndLiquify(in app: XCUIApplication, theme: String = "light") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        #if os(macOS)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        workspaceActivate(app.buttons["color-swap"])
        editorTool("Line", in: app); editorChoice("Rectangle", group: true, in: app); editorChoice("Fill", in: app)
        viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.38, dy: 0.43)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.53, dy: 0.67)))
        editorDocumentTitle(in: app).hover()
        expectation(for: NSPredicate { _, _ in
            let samples = self.editorPixelSamples(in: app, at: [CGPoint(x: 0.48, y: 0.55), CGPoint(x: 0.58, y: 0.55)], size: 8)
            return Int(samples[0][0]) > Int(samples[0][2]) + 80 && Int(samples[1][2]) > Int(samples[1][0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "blend-two-color-fixture-" + theme)
        func pixels() -> Data { editorPixels(in: app, at: CGPoint(x: 0.5, y: 0.5), size: 128) }
        let baseline = pixels()
        let outside = [CGPoint(x: 0.4, y: 0.45), CGPoint(x: 0.65, y: 0.68)]
        let untouched = editorPixelSamples(in: app, at: outside, size: 8)
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        #endif
        for (tool, group, presets, offered) in [("Blend", "Paint mixing", ["Natural Blender"], 5),
            ("Blend", "Blend", ["Smudge"], 8), ("Liquify", "Liquify", ["Liquify Push", "Liquify Twirl Counterclockwise"], 8)] {
            for preset in presets {
                selectPaintPreset(tool, group: group, preset: preset, in: app)
                XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "brush-")).count, offered)
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
                attachEditor(in: app, name: "paint-\(preset)-\(theme)")
                for (command, expected) in [("Undo", baseline), ("Redo", changed), ("Undo", baseline)] {
                    editorHistory(command, in: app); expectPixels(expected)
                }
                #else
                attachEditor(in: app, name: "paint-controls-\(preset)-\(theme)")
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
        tool("Line")
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
        for radial in [false, true] {
            for transparent in [false, true] {
                app.terminate(); useFreshStorage(app)
                let alpha = transparent ? 0 : 1
                app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = "[{\"type\":\"set_theme\",\"theme\":\"light\"},{\"type\":\"set_color\",\"rgba\":[0.9,0.25,0.2,\(alpha)]},{\"type\":\"color\",\"action\":{\"op\":\"swap\"}},{\"type\":\"set_color\",\"rgba\":[0.2,0.45,0.8,1]}]"
                app.launch(); capturePaintEditor(in: app)
                attachEditor(in: app, name: "figure-controls")
                tool("Linear")
                choose(radial ? "Radial" : "Linear", group: false)
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
            "toolbar-tile-toolbar-", "Straight")).firstMatch)
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

    @MainActor func checkStrengthAndSmoothing(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        let viewport = workspaceViewport(in: app), frame = viewport.frame
        let undo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", "Undo")).firstMatch
        let redo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", "Redo")).firstMatch
        func value(_ id: String) -> XCUIElement { app.buttons["number-value-tool-" + id] }
        func expect(_ id: String, _ text: String) {
            expectation(for: NSPredicate(format: "value == %@", text), evaluatedWith: value(id))
            waitForExpectations(timeout: 10)
        }
        func edit(_ id: String, _ text: String) {
            _ = editPaintSetting(id, text: text, in: app)
            expect(id, text + (id == "smoothing_strength" ? "" : " %"))
        }
        let paper = editorPixels(in: app)
        XCTAssertTrue(undo.exists && redo.exists)
        XCTAssertFalse(undo.isEnabled); XCTAssertFalse(redo.isEnabled)
        for (tool, group, preset, id) in [("Pencil", "Pencil", "Pencil", "flow"),
            ("Paint Brush", "Paint", "Paintbrush", "flow"), ("Liquify", "Liquify", "Liquify Push", "strength"),
            ("Paint Brush", "Paint", "Dual Texture", "flow")] {
            selectPaintPreset(tool, group: group, preset: preset, in: app)
            XCTAssertEqual(value(id).label, "Strength")
            XCTAssertEqual(value("opacity").exists, id == "flow")
            if id == "flow" { edit("opacity", "42") }
            for strength in ["0", "37", "100"] {
                edit(id, strength)
                if id == "flow" { expect("opacity", "42 %") }
            }
            if preset == "Dual Texture" { expectPaintGroup("Texture", setting: "grain_depth", in: app) }
            attachEditor(in: app, name: "strength-controls-\(tool)-\(theme)")
        }
        selectPaintPreset("Pen", group: "Pen", preset: "G-Pen", in: app)
        edit("flow", "37"); edit("opacity", "42")
        expect("smoothing_strength", "6")
        let toolScroll = app.scrollViews.containing(.button, identifier: value("smoothing_strength").identifier).firstMatch
        revealEditorControl(value("smoothing_strength"), in: toolScroll)
        XCTAssertEqual(value("smoothing_strength").label, "Smoothing")
        let smoothingTrack = app.descendants(matching: .any)["number-track-tool-smoothing_strength"].firstMatch
        revealEditorControl(smoothingTrack, in: toolScroll)
        let track = smoothingTrack.frame
        XCTAssertTrue(track.width > 0 && track.height > 0 && toolScroll.frame.contains(track))
        smoothingTrack.coordinate(withNormalizedOffset: CGVector(dx: 0.75, dy: 0.5)).clickOrTap()
        expect("smoothing_strength", "30")
        selectPaintPreset("Pencil", group: "Pencil", preset: "Pencil", in: app)
        expect("smoothing_strength", "6")
        selectPaintPreset("Pen", group: "Pen", preset: "G-Pen", in: app)
        expect("smoothing_strength", "30")
        resetPaintSetting("smoothing_strength", in: app)
        expect("smoothing_strength", "6")
        edit("smoothing_strength", "30")
        XCTAssertEqual(editorPixels(in: app), paper, "Tool settings preserve artwork")
        XCTAssertFalse(undo.isEnabled); XCTAssertFalse(redo.isEnabled)
        for strength in ["0", "30"] {
            edit("smoothing_strength", strength)
            attachEditor(in: app, name: "smoothing-controls-\(strength)-\(theme)")
            #if os(macOS)
            editPaintSize("120", in: app)
            let point = CGPoint(x: 0.5, y: 0.52), outside = CGPoint(x: 0.5, y: 0.35)
            let before = editorPixels(in: app, at: point, size: 96), untouched = editorPixels(in: app, at: outside, size: 16)
            viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.44, dy: 0.55)).click(forDuration: 0.05,
                thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.62, dy: 0.55)))
            editorDocumentTitle(in: app).hover()
            expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: point, size: 96) != before }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            let painted = settledEditorPixels(in: app, at: point, size: 96)
            XCTAssertEqual(editorPixels(in: app, at: outside, size: 16), untouched)
            for (command, pixels) in [("Undo", before), ("Redo", painted), ("Undo", before)] {
                editorHistory(command, in: app)
                expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: point, size: 96) == pixels }, evaluatedWith: app)
                waitForExpectations(timeout: 15)
            }
            #endif
        }
        editorTool("Lasso selection", in: app)
        editorChoice("Rectangle select", in: app)
        XCTAssertTrue(value("smoothing_strength").waitForNonExistence(timeout: 10))
        selectPaintPreset("Pen", group: "Pen", preset: "G-Pen", in: app)
        expect("smoothing_strength", "30")
        editorMenu(in: app, menu: "Select", id: "quick_mask", label: "Quick Mask")
        let quick = app.descendants(matching: .any)["layer-row-0"].firstMatch
        XCTAssertTrue(quick.waitForExistence(timeout: 10))
        XCTAssertTrue(value("smoothing_strength").waitForNonExistence(timeout: 10))
        attachEditor(in: app, name: "quick-mask-smoothing-hidden-\(theme)")
        editorMenu(in: app, menu: "Select", id: "quick_mask", label: "Quick Mask")
        XCTAssertTrue(quick.waitForNonExistence(timeout: 10))
        expect("smoothing_strength", "30")
        XCTAssertEqual(viewport.frame, frame)
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
