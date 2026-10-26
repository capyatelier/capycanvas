import XCTest

extension XCTestCase {
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
