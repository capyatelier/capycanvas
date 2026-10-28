import XCTest
#if os(macOS)
import AppKit
#endif

extension XCTestCase {
    #if os(macOS)
    @MainActor func checkVectorKeyboard(in app: XCUIApplication, theme: String) throws {
        let clipboard = try NativePhotoPasteboard()
        let marker = NSPasteboardItem()
        marker.setString(UUID().uuidString, forType: .string)
        try clipboard.replace([marker])
        defer { clipboard.restore() }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.1,0.1,0.12,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        createWhitePaperDrawing(width: "1440", height: "1080", in: app)
        workspaceActivate(app.buttons["panel-tab-layers"])
        workspaceActivate(app.buttons["layer-new-vector"])
        let viewport = workspaceViewport(in: app), paper = editorPaper(in: app)
        func coordinate(_ x: CGFloat, _ y: CGFloat) -> XCUICoordinate {
            viewport.coordinate(withNormalizedOffset: paper.offset(x, y))
        }
        func drag(_ from: (CGFloat, CGFloat), _ to: (CGFloat, CGFloat), modifiers: XCUIElement.KeyModifierFlags = []) {
            XCUIElement.perform(withKeyModifiers: modifiers) {
                coordinate(from.0, from.1).click(forDuration: 0.05, thenDragTo: coordinate(to.0, to.1))
            }
            editorDocumentTitle(in: app).hover()
        }
        func tap(_ x: CGFloat, _ y: CGFloat, modifiers: XCUIElement.KeyModifierFlags = []) {
            XCUIElement.perform(withKeyModifiers: modifiers) { coordinate(x, y).click() }
            editorDocumentTitle(in: app).hover()
        }
        func key(_ value: XCUIKeyboardKey, modifiers: XCUIElement.KeyModifierFlags = []) {
            app.typeKey(value.rawValue, modifierFlags: modifiers)
        }
        func expectInk(_ points: [(CGFloat, CGFloat)], _ wanted: [Bool], _ name: String) {
            let locations = points.map { point -> CGPoint in
                let center = paper.point(point.0, point.1)
                return CGPoint(x: center.x - 2 / viewport.frame.width, y: center.y - 2 / viewport.frame.height)
            }
            let match = NSPredicate { _, _ in
                self.editorPixelSamples(in: app, at: locations, size: 8).map { bytes in
                    stride(from: 0, to: bytes.count, by: 4).contains { max(bytes[$0], bytes[$0 + 1], bytes[$0 + 2]) < 100 }
                } == wanted
            }
            if XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: match, object: app)], timeout: 10) != .completed {
                attachEditor(in: app, name: "vector-keyboard-\(name)-\(theme)")
                XCTFail("Unexpected vector artwork after \(name)")
            }
        }
        editorTool("Line", in: app); editorChoice("Path", group: true, in: app); editorChoice("Outline", in: app)
        for point: (CGFloat, CGFloat) in [(0.2, 0.82), (0.4, 0.82), (0.5, 0.92)] { tap(point.0, point.1) }
        key(.forwardDelete)
        activateCanvasBarAction("finish_path", label: "Finish path", in: app)
        let finish = app.buttons["canvas-bar-action-finish_path"]
        XCTAssertTrue(finish.waitForNonExistence(timeout: 10))
        expectInk([(0.3, 0.82), (0.45, 0.87)], [true, true], "Forward Delete preserves Path draft")
        editorHistory("Undo", in: app)
        for point: (CGFloat, CGFloat) in [(0.2, 0.82), (0.4, 0.82), (0.5, 0.92)] { tap(point.0, point.1) }
        key(.delete); key(.return)
        XCTAssertTrue(finish.waitForNonExistence(timeout: 10))
        expectInk([(0.3, 0.82), (0.45, 0.87)], [true, false], "Backspace and Enter Path keys")
        editorHistory("Undo", in: app)
        tap(0.2, 0.82); tap(0.4, 0.82); key(.escape)
        expectInk([(0.3, 0.82)], [false], "Escape cancels Path")
        editorChoice("Rectangle", group: true, in: app); editorChoice("Outline", in: app)
        editPaintSize("16", in: app); drag((0.2, 0.2), (0.4, 0.4))
        editPaintSize("32", in: app); drag((0.6, 0.2), (0.8, 0.4))
        editorTool("Operation", in: app); editorChoice("Operation", group: true, in: app)
        drag((0.25, 0.2), (0.25, 0.5))
        expectInk([(0.3, 0.2), (0.3, 0.5)], [false, true], "Move drag")

        let sample = paper.point(0.25, 0.5)
        let beforeNudge = editorPixels(in: app, at: sample, size: 24)
        for modifiers in [XCUIElement.KeyModifierFlags(), .shift] {
            key(.downArrow, modifiers: modifiers)
            expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 24) != beforeNudge }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 24) == beforeNudge }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }

        drag((0.25, 0.5), (0.25, 0.75), modifiers: .option)
        expectInk([(0.3, 0.5), (0.3, 0.75)], [true, true], "Option-drag copy")
        key(.delete)
        expectInk([(0.3, 0.5), (0.3, 0.75)], [true, false], "Delete copied path")
        tap(0.25, 0.5)
        let previous = clipboard.board.changeCount
        app.typeKey("c", modifierFlags: .command)
        expectation(for: NSPredicate { _, _ in
            clipboard.board.changeCount != previous && clipboard.board.data(forType: .png).map { NSImage(data: $0) != nil } == true
        }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        clipboard.changeCount = clipboard.board.changeCount
        let edit = app.menuBars.menuBarItems["Edit"]
        let paste = edit.descendants(matching: .menuItem)["Paste"].firstMatch
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: paste)
        waitForExpectations(timeout: 10)
        app.typeKey("v", modifierFlags: .command)
        let copy = edit.descendants(matching: .menuItem)["Copy"].firstMatch
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: copy)
        waitForExpectations(timeout: 10)
        drag((0.25, 0.5), (0.25, 0.75))
        expectInk([(0.3, 0.5), (0.3, 0.75)], [true, true], "Cmd-C Cmd-V")
        key(.delete)
        expectInk([(0.3, 0.5), (0.3, 0.75)], [true, false], "Delete pasted path")

        tap(0.1, 0.1)
        drag((0.56, 0.16), (0.84, 0.44))
        tap(0.25, 0.5, modifiers: .shift)
        key(.delete)
        expectInk([(0.3, 0.5), (0.7, 0.2)], [false, false], "marquee and Shift selection")
        editorHistory("Undo", in: app)
        expectInk([(0.3, 0.5), (0.7, 0.2)], [true, true], "Undo selection delete")
        tap(0.1, 0.1); tap(0.25, 0.5)
        drag((0.4 + 8.0 / 1440, 0.7 + 8.0 / 1080), (0.5, 0.8))
        expectInk([(0.3, 0.7), (0.3, 0.8)], [false, true], "frame scale")
        editorHistory("Undo", in: app)
        let top = coordinate(0.3, 0.5 - 8.0 / 1080)
        let rotation = top.withOffset(CGVector(dx: 0, dy: -30))
        let center = coordinate(0.3, 0.6).screenPoint
        let radius = center.y - rotation.screenPoint.y
        rotation.click(forDuration: 0.05, thenDragTo: rotation.withOffset(CGVector(dx: radius / sqrt(2), dy: radius * (1 - 1 / sqrt(2)))))
        editorDocumentTitle(in: app).hover()
        expectInk([(0.3, 0.5), (0.35303, 0.52929)], [false, true], "frame rotate")
        editorHistory("Undo", in: app)

        tap(0.25, 0.5); tap(0.65, 0.2, modifiers: .shift)
        let readout = app.buttons["number-value-property-path_width"]
        if !readout.exists {
            let tab = app.buttons["panel-tab-properties"]
            workspaceActivate(tab.exists ? tab : app.buttons["column-icon-properties"])
        }
        revealEditorControl(readout, identifier: "number-value-property-path_width", type: .button, in: app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch)
        XCTAssertTrue(app.staticTexts["Outline width (mixed)"].exists)
        workspaceActivate(readout)
        let width = app.textFields["number-entry-property-path_width"]
        XCTAssertTrue(width.waitForExistence(timeout: 10)); width.typeText("24\n")
        XCTAssertTrue(width.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["Outline width (mixed)"].waitForNonExistence(timeout: 10))
        for point: (CGFloat, CGFloat) in [(0.25, 0.5), (0.65, 0.2)] {
            tap(0.1, 0.1); tap(point.0, point.1)
            XCTAssertTrue(readout.waitForExistence(timeout: 10))
            expectation(for: NSPredicate(format: "value BEGINSWITH %@", "24"), evaluatedWith: readout)
            waitForExpectations(timeout: 5)
        }

        attachEditor(in: app, name: "vector-keyboard-\(theme)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
    #endif

    @MainActor func checkVectorLayers(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.15,0.7,0.25,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.1,0.1,0.12,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        #if os(macOS)
        createWhitePaperDrawing(in: app)
        #endif
        let layers = app.buttons["panel-tab-layers"]
        XCTAssertTrue(layers.waitForExistence(timeout: 10))
        if !layers.isSelected { workspaceActivate(layers) }
        let rows = artworkRows(in: app)
        let vectors = rows.matching(NSPredicate(format: "label BEGINSWITH %@", "Vector "))
        let activeVector = vectors.containing(NSPredicate(format: "identifier == %@ AND selected == YES", "layer-Select layer without changing drawing target"))
        func expectCount(_ query: XCUIElementQuery, _ expected: Int) {
            expectation(for: NSPredicate(format: "count == %d", expected), evaluatedWith: query)
            waitForExpectations(timeout: 20)
        }
        func brush(_ label: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "brush-", label)).firstMatch
        }
        func expectBrush(_ label: String) {
            XCTAssertTrue(brush(label).waitForExistence(timeout: 10), "The Brushes panel offers \(label)")
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: brush(label))
            waitForExpectations(timeout: 10)
        }
        func newVectorLayer() {
            let total = rows.count, paths = vectors.count
            let button = app.buttons["layer-new-vector"]
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            XCTAssertEqual(button.label, "New vector layer")
            workspaceActivate(button)
            expectCount(rows, total + 1); expectCount(vectors, paths + 1); expectCount(activeVector, 1)
        }
        func acceptNotice(_ token: String, _ label: String) {
            let action = app.buttons["canvas-notice-action-" + token]
            XCTAssertTrue(action.waitForExistence(timeout: 10), "The notice offers \(label)")
            XCTAssertEqual(action.label, label)
            action.clickOrTap()
            XCTAssertTrue(app.descendants(matching: .any)["canvas-notice"].firstMatch.waitForNonExistence(timeout: 5))
        }
        #if os(macOS)
        let viewport = workspaceViewport(in: app), paper = editorPaper(in: app)
        let lift = CGVector(dx: 6 / viewport.frame.width, dy: 6 / viewport.frame.height)
        let selectedContent = NSPredicate(format: "identifier BEGINSWITH %@ AND identifier ENDSWITH %@ AND selected == YES", "layer-thumbnail-", "-content")
        let drawingTarget = rows.containing(selectedContent)
        func coordinate(_ point: (CGFloat, CGFloat)) -> XCUICoordinate {
            viewport.coordinate(withNormalizedOffset: paper.offset(point.0, point.1))
        }
        func drag(_ from: (CGFloat, CGFloat), _ to: (CGFloat, CGFloat), leaveCanvas: Bool = true) {
            coordinate(from).click(forDuration: 0.05, thenDragTo: coordinate(to))
            if leaveCanvas { editorDocumentTitle(in: app).hover() }
        }
        func tap(_ point: (CGFloat, CGFloat)) {
            coordinate(point).click()
            editorDocumentTitle(in: app).hover()
        }
        func expect(_ name: String, _ ink: @escaping (Int, Int, Int) -> Bool,
                    at present: [(CGFloat, CGFloat)], not absent: [(CGFloat, CGFloat)] = []) {
            let points = (present + absent).map {
                let center = paper.point($0.0, $0.1)
                return CGPoint(x: center.x - lift.dx, y: center.y - lift.dy)
            }
            let wanted = Array(repeating: true, count: present.count) + Array(repeating: false, count: absent.count)
            func inked(_ sample: Data) -> Bool {
                stride(from: 0, to: sample.count, by: 4).contains { (i: Int) -> Bool in
                    ink(Int(sample[i]), Int(sample[i + 1]), Int(sample[i + 2]))
                }
            }
            let presented = NSPredicate { _, _ in self.editorPixelSamples(in: app, at: points, size: 24).map(inked) == wanted }
            let match = XCTNSPredicateExpectation(predicate: presented, object: app)
            if XCTWaiter.wait(for: [match], timeout: 20) != .completed {
                attachEditor(in: app, name: "vector-\(name)-\(theme)")
                XCTFail("Unexpected presented pixels: \(name) in the \(theme) theme")
            }
        }
        let dark: (Int, Int, Int) -> Bool = { max($0, $1, $2) < 100 }
        let marked: (Int, Int, Int) -> Bool = { $0 + $1 + $2 < 600 }
        let green: (Int, Int, Int) -> Bool = { $1 - max($0, $2) > 40 }
        #endif

        selectPaintPreset("Pencil", group: "Pencil", preset: "Pencil", in: app)
        newVectorLayer()
        expectBrush("G-Pen")
        attachEditor(in: app, name: "vector-new-layer-\(theme)")
        #if os(macOS)
        editPaintSize("16", in: app)
        drag((0.15, 0.12), (0.45, 0.14))
        expect("discover", dark, at: [(0.3, 0.13)])

        selectPaintPreset("Pencil", group: "Pencil", preset: "Pencil", in: app)
        drag((0.15, 0.22), (0.45, 0.24), leaveCanvas: false)
        acceptNotice("use_vector_pen", "Use G-Pen")
        expectBrush("G-Pen")
        editorDocumentTitle(in: app).hover()
        expect("refused-pencil", marked, at: [], not: [(0.3, 0.23)])
        drag((0.15, 0.22), (0.45, 0.24))
        expect("pen-line", dark, at: [(0.3, 0.23)])

        selectPaintPreset("Pencil", group: "Pencil", preset: "Pencil", in: app)
        let beforePaint = rows.count
        drag((0.15, 0.32), (0.45, 0.34), leaveCanvas: false)
        acceptNotice("new_paint_layer", "New Paint Layer")
        expectCount(rows, beforePaint + 1); expectCount(activeVector, 0); expectCount(drawingTarget, 1)
        XCTAssertFalse(drawingTarget.firstMatch.label.hasPrefix("Vector "))
        expectBrush("Pencil")
        editPaintSize("30", in: app)
        expect("refused-pencil-on-paint", marked, at: [], not: [(0.3, 0.33)])
        drag((0.15, 0.32), (0.45, 0.34))
        expect("pencil-on-paint", marked, at: [(0.3, 0.33)])
        attachEditor(in: app, name: "vector-wrong-brush-\(theme)")

        newVectorLayer()
        expectBrush("G-Pen")
        drag((0.1, 0.55), (0.9, 0.55))
        drag((0.3, 0.45), (0.3, 0.65))
        drag((0.7, 0.45), (0.7, 0.65))
        expect("crossing-lines", dark, at: [(0.2, 0.55), (0.5, 0.55), (0.8, 0.55), (0.3, 0.48), (0.7, 0.48)])
        #endif

        editorTool("Eraser", in: app)
        let modes = ["Erase touched part", "Erase whole paths", "Erase up to intersection"]
        let subtools = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "tool-subtool-"))
        expectation(for: NSPredicate { _, _ in subtools.allElementsBoundByIndex.prefix(3).map(\.label) == modes }, evaluatedWith: subtools)
        waitForExpectations(timeout: 10)
        editorChoice("Erase up to intersection", in: app)
        attachEditor(in: app, name: "vector-eraser-modes-\(theme)")
        #if os(macOS)
        editPaintSize("16", in: app)
        drag((0.5, 0.52), (0.5, 0.58))
        expect("erase-to-intersection", dark, at: [(0.2, 0.55), (0.8, 0.55), (0.3, 0.48), (0.7, 0.48)],
            not: [(0.4, 0.55), (0.5, 0.55), (0.6, 0.55)])
        attachEditor(in: app, name: "vector-erased-\(theme)")
        #endif

        let vectorRow = activeVector.firstMatch.identifier
        let order = rows.allElementsBoundByIndex.map(\.identifier)
        guard let index = order.firstIndex(of: vectorRow), index + 1 < order.count else {
            return XCTFail("The vector layer needs a paint layer below it")
        }
        let belowRow = rows[order[index + 1]]
        workspaceActivate(belowRow.buttons["Edit layer content"])
        expectCount(activeVector, 0)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: belowRow.buttons["Edit layer content"])
        waitForExpectations(timeout: 10)
        workspaceActivate(rows[vectorRow].buttons["Edit layer content"])
        expectCount(activeVector, 1)
        let beforeMerge = rows.count, paths = vectors.count
        editorMenu(in: app, menu: "Layer", id: "merge_down", label: "Merge Down")
        acceptNotice("merge", "Merge Down")
        expectCount(rows, beforeMerge - 1); expectCount(vectors, paths - 1); expectCount(activeVector, 0)
        attachEditor(in: app, name: "vector-merged-\(theme)")
        #if os(macOS)
        expect("merged", dark, at: [(0.2, 0.55), (0.8, 0.55)], not: [(0.5, 0.55)])

        newVectorLayer()
        editorTool("Line", in: app)
        editorChoice("Ellipse", group: true, in: app); editorChoice("Outline", in: app)
        drag((0.58, 0.08), (0.92, 0.38))
        expect("ellipse-outline", marked, at: [(0.75, 0.08)], not: [(0.75, 0.23)])
        workspaceActivate(app.buttons["color-swap"])
        editorTool("Fill", in: app); editorChoice("Visible artwork", in: app)
        let beforeFill = rows.count
        tap((0.75, 0.23))
        expect("filled-inside", green, at: [(0.75, 0.23)], not: [(0.85, 0.75)])
        XCTAssertEqual(rows.count, beforeFill, "Filling inside an outline colors its path")
        tap((0.85, 0.75))
        expectCount(rows, beforeFill + 1); expectCount(activeVector, 0)
        expect("filled-below", green, at: [(0.85, 0.75), (0.75, 0.23)])
        attachEditor(in: app, name: "vector-color-\(theme)")
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
