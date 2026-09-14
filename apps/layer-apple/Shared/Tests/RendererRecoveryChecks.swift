import XCTest
import CoreGraphics

extension XCTestCase {
    @MainActor func checkRendererRecovery(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_GPU_RECOVERY_TEST"] = "1"
        app.launchEnvironment["CAPY_CAPTURE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"add_layer"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"invoke","command":"select_all"},{"type":"invoke","command":"fill_selection"}]"#
        app.launch()
        let status = app.staticTexts["renderer-test-status"]
        func waitFor(_ text: String) {
            expectation(for: NSPredicate(format: "label == %@ OR value == %@", text, text), evaluatedWith: status)
            waitForExpectations(timeout: 30)
        }
        func screenshot() -> XCUIScreenshot {
            #if os(macOS)
            return app.windows.firstMatch.screenshot()
            #else
            // Application captures can crop landscape iPad content using a
            // portrait rectangle. This fixture owns the full screen.
            return XCUIScreen.main.screenshot()
            #endif
        }
        func pixels() -> Data {
            XCTAssertEqual(app.state, .runningForeground, "The review editor must remain visible for pixel checks")
            #if os(macOS)
            let image = screenshot().image.cgImage(forProposedRect: nil, context: nil, hints: nil)!
            #else
            let image = screenshot().image.cgImage!
            #endif
            let crop = image.cropping(to: CGRect(x: Double(image.width) * 0.6, y: Double(image.height) * 0.6, width: 8, height: 8))!
            var data = Data(count: 8 * 8 * 4)
            data.withUnsafeMutableBytes { bytes in
                let context = CGContext(data: bytes.baseAddress, width: 8, height: 8, bitsPerComponent: 8, bytesPerRow: 32,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                context.draw(crop, in: CGRect(x: 0, y: 0, width: 8, height: 8))
            }
            return data
        }
        func command(_ name: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", name)).firstMatch
        }
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func waitForThumbnails() {
            let thumbnails = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-thumbnail-"))
            expectation(for: NSPredicate { _, _ in
                thumbnails.count > 0 && thumbnails.allElementsBoundByIndex.allSatisfy { $0.value as? String == "Preview ready" }
            }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
        }
        waitFor("Renderer ready")
        waitForThumbnails()
        let initial = XCTAttachment(screenshot: screenshot())
        initial.name = "renderer-before-failure"; initial.lifetime = .keepAlways; add(initial)
        expectation(for: NSPredicate { _, _ in let sample = pixels(); return Int(sample[2]) > Int(sample[0]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 20)
        let painted = pixels()
        for fault in ["Lose test device", "Validate test failure"] {
            workspaceActivate(app.buttons[fault])
            // The native display link is idle here. The owner health check must
            // still deliver loss, without another touch or forced frame.
            waitFor("Renderer stopped")
            XCTAssertTrue(app.staticTexts["Canvas error"].exists)
            XCTAssertFalse(command("Undo").isEnabled)
            XCTAssertTrue(app.buttons["Save As…"].firstMatch.isEnabled)
            workspaceActivate(app.buttons["Restart Canvas"])
            waitFor("Renderer ready")
            expectPixels(painted)
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            workspaceActivate(command("Undo"))
            expectation(for: NSPredicate { _, _ in pixels() != painted }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            workspaceActivate(command("Redo"))
            expectPixels(painted)
            waitForThumbnails()
            let capture = XCTAttachment(screenshot: screenshot())
            capture.name = "renderer-recovered-" + fault; capture.lifetime = .keepAlways; add(capture)
        }
        workspaceActivate(app.buttons["layer-New layer"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
