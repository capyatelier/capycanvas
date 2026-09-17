import XCTest
import ImageIO
import UniformTypeIdentifiers

extension XCTestCase {
    @MainActor func chooseExportDestination(in app: XCUIApplication) {
        let choose = app.buttons["export-choose-file"]
        XCTAssertTrue(choose.waitForExistence(timeout: 15))
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: choose)
        waitForExpectations(timeout: 30); workspaceActivate(choose)
    }
}

#if os(macOS)
extension XCTestCase {
    @MainActor func checkNativeProfiledExport(in app: XCUIApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Export " + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let photo = root.appendingPathComponent("Original.png"), output = root.appendingPathComponent("Delivery.tiff")
        let profile = root.appendingPathComponent("Delivery.icc")
        let space = try XCTUnwrap(CGColorSpace(name: CGColorSpace.displayP3))
        let icc = try XCTUnwrap(space.copyICCData()) as Data
        try icc.write(to: profile)
        let context = try XCTUnwrap(CGContext(data: nil, width: 128, height: 96, bitsPerComponent: 8,
            bytesPerRow: 512, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(red: 0.7, green: 0.3, blue: 0.15, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: 128, height: 96))
        let file = try XCTUnwrap(CGImageDestinationCreateWithURL(photo as CFURL, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(file, try XCTUnwrap(context.makeImage()), nil)
        XCTAssertTrue(CGImageDestinationFinalize(file))
        let original = try Data(contentsOf: photo)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        func chooseFile(_ url: URL) {
            let open = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(open.waitForExistence(timeout: 15))
            app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(url.path + "\n")
            workspaceActivate(open); XCTAssertTrue(open.waitForNonExistence(timeout: 15))
        }
        editorMenu(in: app, menu: "File", id: "open_document", label: "Open…"); chooseFile(photo)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let before = editorPixels(in: app)
        func openExport() {
            editorMenu(in: app, menu: "File", id: "export_document", label: "Export…")
            let choose = app.buttons["export-choose-file"]
            XCTAssertTrue(choose.waitForExistence(timeout: 15))
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: choose); waitForExpectations(timeout: 30)
        }
        func reveal(_ element: XCUIElement) {
            let scroll = app.scrollViews.containing(.button, identifier: "export-preview").firstMatch
            XCTAssertTrue(element.waitForExistence(timeout: 10))
            // Edge-aligned controls need no decorative inset to be clickable.
            for _ in 0..<15 {
                let center = CGPoint(x: element.frame.midX, y: element.frame.midY)
                if scroll.frame.contains(center) { return }
                scroll.scroll(byDeltaX: 0, deltaY: center.y > scroll.frame.maxY ? -100 : 100)
            }
            XCTFail("Export control is outside the viewport: \(element.identifier)")
        }
        func choice(_ id: String, _ label: String) {
            let picker = app.descendants(matching: .any).matching(identifier: id).firstMatch
            reveal(picker); workspaceActivate(picker); workspaceActivate(app.menuItems[label].firstMatch)
        }
        func field(_ id: String, _ value: String) {
            let field = app.textFields[id]; reveal(field); workspaceActivate(field)
            field.typeKey("a", modifierFlags: .command); field.typeText(value)
        }
        openExport(); choice("export-destination", "Further editing")
        choice("export-format", "JPEG")
        let depth = app.descendants(matching: .any).matching(identifier: "export-depth").firstMatch
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: depth)
        waitForExpectations(timeout: 15)
        choice("export-format", "TIFF")
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: depth)
        waitForExpectations(timeout: 15)
        choice("export-destination", "Further editing")
        let importProfile = app.buttons["source-profile-import"]
        reveal(importProfile); workspaceActivate(importProfile)
        chooseFile(URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"))
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: app.buttons["export-choose-file"])
        waitForExpectations(timeout: 20)
        for (id, excluded) in [("export-format", "PNG"), ("export-background", "Preserve")] {
            let picker = app.descendants(matching: .any).matching(identifier: id).firstMatch
            reveal(picker); workspaceActivate(picker)
            XCTAssertFalse(app.menuItems[excluded].exists, "CMYK must omit unsupported \(excluded)")
            app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        }
        reveal(importProfile); workspaceActivate(importProfile); chooseFile(profile)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: app.buttons["export-choose-file"])
        waitForExpectations(timeout: 20)
        let fit = app.checkBoxes["export-fit"]; reveal(fit); workspaceActivate(fit)
        field("export-width", "64"); field("export-height", "64")
        let preview = app.buttons["export-preview"]
        reveal(preview); workspaceActivate(preview)
        XCTAssertTrue(app.staticTexts["export-output-size"].waitForExistence(timeout: 60))
        let size = app.staticTexts["export-output-size"]
        XCTAssertEqual(size.value as? String ?? size.label, "Output: 64 × 48 pixels")
        reveal(size); attachEditor(in: app, name: "profiled-export-comparison")
        let presets = app.disclosureTriangles["Saved presets"].firstMatch
        reveal(presets)
        // AppKit reports the disclosure hit point left of the clipped scroll
        // content. Click its visible chevron using the measured content edge.
        let scroll = app.scrollViews.containing(.button, identifier: "export-preview").firstMatch
        scroll.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 6, dy: presets.frame.midY - scroll.frame.minY)).click()
        field("export-preset-name", "Studio TIFF")
        let savePreset = app.buttons["export-preset-save"]; reveal(savePreset); workspaceActivate(savePreset)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: app.buttons["export-choose-file"])
        waitForExpectations(timeout: 20)
        chooseExportDestination(in: app)
        let save = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(save.waitForExistence(timeout: 15))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(save.waitForNonExistence(timeout: 10)); XCTAssertEqual(editorPixels(in: app), before)
        openExport(); choice("export-destination", "Studio TIFF")
        let library = app.buttons["color-profile-library"]; reveal(library); workspaceActivate(library)
        let use = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "profile-use-")).firstMatch
        XCTAssertTrue(use.waitForExistence(timeout: 15)); attachEditor(in: app, name: "saved-icc-profile-library")
        workspaceActivate(use); XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        chooseExportDestination(in: app)
        XCTAssertTrue(save.waitForExistence(timeout: 15))
        app.typeKey("g", modifierFlags: [.command, .shift]); app.typeText(root.path + "\n")
        let name = app.textFields["saveAsNameTextField"]; workspaceActivate(name)
        name.typeKey("a", modifierFlags: .command); name.typeText(output.lastPathComponent)
        workspaceActivate(save); XCTAssertTrue(save.waitForNonExistence(timeout: 15))
        expectation(for: NSPredicate { _,_ in FileManager.default.fileExists(atPath: output.path) }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        let result = try XCTUnwrap(CGImageSourceCreateWithURL(output as CFURL, nil))
        let image = try XCTUnwrap(CGImageSourceCreateImageAtIndex(result, 0, nil))
        XCTAssertEqual(image.bitsPerComponent, 16); XCTAssertEqual(image.width, 64); XCTAssertEqual(image.height, 48)
        XCTAssertNotNil(image.colorSpace?.copyICCData())
        XCTAssertEqual(try Data(contentsOf: photo), original); XCTAssertEqual(try Data(contentsOf: profile), icc)
        XCTAssertEqual(editorPixels(in: app), before)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
#endif
