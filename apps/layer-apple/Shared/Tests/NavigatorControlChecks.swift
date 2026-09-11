import XCTest
import ImageIO

extension XCTestCase {
    @MainActor func checkNavigatorAndDiagnostics(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 10))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        func text(_ element: XCUIElement) -> String {
            if let value = element.value as? String, !value.isEmpty { return value }
            return element.label
        }
        func pixels(_ element: XCUIElement, _ name: String) -> Data {
            let screenshot = element.screenshot()
            let attachment = XCTAttachment(screenshot: screenshot)
            attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
            guard let source = CGImageSourceCreateWithData(screenshot.pngRepresentation as CFData, nil),
                let image = CGImageSourceCreateImageAtIndex(source, 0, nil), let data = image.dataProvider?.data else {
                XCTFail("Navigator capture must contain actual composited pixels"); return Data()
            }
            return data as Data
        }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let overview = app.descendants(matching: .any)["navigator-overview"].firstMatch
        XCTAssertTrue(overview.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "Live preview"), evaluatedWith: overview)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(workspaceViewport(in: app).frame.contains(overview.frame))
        let blank = pixels(overview, "navigator-before-ink")
        let viewport = workspaceViewport(in: app)
        let inkStart = viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.4, dy: 0.55))
        let inkEnd = viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.48, dy: 0.6))
        #if os(macOS)
        inkStart.click(forDuration: 0.1, thenDragTo: inkEnd)
        #else
        inkStart.press(forDuration: 0.1, thenDragTo: inkEnd)
        #endif
        func command(_ name: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label BEGINSWITH %@", "toolbar-tile-", name)).firstMatch
        }
        let undo = command("Undo"), redo = command("Redo")
        #if os(macOS)
        expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: undo)
        waitForExpectations(timeout: 5)
        let painted = pixels(overview, "navigator-painted")
        XCTAssertNotEqual(painted, blank, "Ink must reach the native live overview after pen-up")
        activate(undo)
        expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: redo)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(pixels(overview, "navigator-undone"), blank, "Undo must restore the overview's actual pixels")
        activate(redo)
        expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: undo)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(pixels(overview, "navigator-redone"), painted, "Redo must restore the final ink in the overview")
        #else
        // XCTest delivers a finger here. The shared single-finger policy must
        // keep ink unchanged; it cannot stand in for a physical Pencil check.
        XCTAssertFalse(undo.isEnabled)
        XCTAssertEqual(pixels(overview, "navigator-after-finger"), blank)
        #endif
        let status = app.staticTexts["camera-status"]
        let original = text(status)
        XCTAssertFalse(original.isEmpty, "The native camera readout must be accessible")
        activate(app.buttons["navigator-zoom_in"])
        expectation(for: NSPredicate { _, _ in text(status) != original }, evaluatedWith: status)
        waitForExpectations(timeout: 5)
        activate(app.buttons["navigator-rotate_right"])
        activate(app.buttons["navigator-flip_horizontal"])
        let start = overview.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = overview.coordinate(withNormalizedOffset: CGVector(dx: 0.62, dy: 0.55))
        #if os(macOS)
        start.click(forDuration: 0.1, thenDragTo: end)
        #else
        start.press(forDuration: 0.1, thenDragTo: end)
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        activate(app.buttons["panel-tab-stats"])
        let frames = app.staticTexts["stats-value-2"]
        XCTAssertTrue(frames.waitForExistence(timeout: 10))
        XCTAssertGreaterThan(Int(text(frames)) ?? 0, 0)
        activate(app.buttons["panel-tab-navigator"])
        XCTAssertTrue(overview.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "Live preview"), evaluatedWith: overview)
        waitForExpectations(timeout: 10)
        #if os(macOS)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #else
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        #endif
        shot.name = "navigator-controls"; shot.lifetime = .keepAlways; add(shot)
    }
}
