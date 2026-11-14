import XCTest
import UIKit

@MainActor var phoneDrawingFixtures: [String: [String: Any]] = [:]

private enum PhoneMenuGroup {
    case fileToLayer, selectToWorkspaces, search
    var menus: [String] {
        switch self {
        case .fileToLayer: ["file", "edit", "image", "layer"]
        case .selectToWorkspaces: ["select", "filter", "view", "window"]
        case .search: []
        }
    }
}

extension EditorLaunchTests {
    @MainActor func testPhoneLayoutLight() throws { try checkPhoneLayout(theme: "light") }
    @MainActor func testPhoneLayoutDark() throws { try checkPhoneLayout(theme: "dark") }

    @MainActor func testPhoneMenusLight() throws { try checkPhoneMenus(theme: "light", orientation: .portrait, swapped: false, group: .fileToLayer) }
    @MainActor func testPhoneMenusDark() throws { try checkPhoneMenus(theme: "dark", orientation: .portrait, swapped: false, group: .fileToLayer) }
    @MainActor func testPhoneMenusSwappedLight() throws { try checkPhoneMenus(theme: "light", orientation: .portrait, swapped: true, group: .fileToLayer) }
    @MainActor func testPhoneMenusSwappedDark() throws { try checkPhoneMenus(theme: "dark", orientation: .portrait, swapped: true, group: .fileToLayer) }
    @MainActor func testPhoneMenusSelectionLight() throws { try checkPhoneMenus(theme: "light", orientation: .portrait, swapped: false, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusSelectionDark() throws { try checkPhoneMenus(theme: "dark", orientation: .portrait, swapped: false, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusSelectionSwappedLight() throws { try checkPhoneMenus(theme: "light", orientation: .portrait, swapped: true, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusSelectionSwappedDark() throws { try checkPhoneMenus(theme: "dark", orientation: .portrait, swapped: true, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusSearchLight() throws { try checkPhoneMenus(theme: "light", orientation: .portrait, swapped: false, group: .search) }
    @MainActor func testPhoneMenusSearchDark() throws { try checkPhoneMenus(theme: "dark", orientation: .portrait, swapped: false, group: .search) }
    @MainActor func testPhoneMenusLandscapeLeftLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeLeft, swapped: false, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeLeftDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeLeft, swapped: false, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeLeftSwappedLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeLeft, swapped: true, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeLeftSwappedDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeLeft, swapped: true, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeLeftSelectionLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeLeft, swapped: false, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeLeftSelectionDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeLeft, swapped: false, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeLeftSelectionSwappedLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeLeft, swapped: true, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeLeftSelectionSwappedDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeLeft, swapped: true, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeLeftSearchLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeLeft, swapped: false, group: .search) }
    @MainActor func testPhoneMenusLandscapeLeftSearchDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeLeft, swapped: false, group: .search) }
    @MainActor func testPhoneMenusLandscapeRightLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeRight, swapped: false, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeRightDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeRight, swapped: false, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeRightSwappedLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeRight, swapped: true, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeRightSwappedDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeRight, swapped: true, group: .fileToLayer) }
    @MainActor func testPhoneMenusLandscapeRightSelectionLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeRight, swapped: false, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeRightSelectionDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeRight, swapped: false, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeRightSelectionSwappedLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeRight, swapped: true, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeRightSelectionSwappedDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeRight, swapped: true, group: .selectToWorkspaces) }
    @MainActor func testPhoneMenusLandscapeRightSearchLight() throws { try checkPhoneMenus(theme: "light", orientation: .landscapeRight, swapped: false, group: .search) }
    @MainActor func testPhoneMenusLandscapeRightSearchDark() throws { try checkPhoneMenus(theme: "dark", orientation: .landscapeRight, swapped: false, group: .search) }

    @MainActor func testPhoneFrameLight() throws { try checkPhoneFrame(theme: "light") }
    @MainActor func testPhoneFrameDark() throws { try checkPhoneFrame(theme: "dark") }

    @MainActor func testPhoneInputLight() throws { try checkPhoneInput(theme: "light") }
    @MainActor func testPhoneInputDark() throws { try checkPhoneInput(theme: "dark") }

    @MainActor private func checkPhoneInput(theme: String) throws {
        for orientation in phoneCaptureOrientations {
            requestedPhoneOrientation = orientation
            let actions: [[String: Any]] = [["type": "invoke", "command": "drawing_brush"],
                ["type": "set_brush_size", "value": 40], ["type": "set_color", "rgba": [0, 0, 0, 1]],
                ["type": "preferences", "action": ["type": "edit", "id": "finger_drawing", "value": 0]]]
            let app = try phoneCaptureApplication(theme: theme)
            defer { app.terminate() }
            phoneCapture(in: app, id: phoneCaptureID("input", "rest"), theme: theme, fixture: [])
            let initial = try XCTUnwrap(app.launchEnvironment["CAPY_INITIAL_ACTIONS"]?.data(using: .utf8))
            let baseline = try XCTUnwrap(try JSONSerialization.jsonObject(with: initial) as? [[String: Any]])
            try phoneRelaunch(app, theme: theme, actions: baseline + actions)
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            let region = try XCTUnwrap(phoneDrawerCanvasRegion(in: app)), bounds = canvas.frame
            let sample = CGPoint(x: (region.midX - bounds.minX) / bounds.width, y: (region.midY - bounds.minY) / bounds.height)
            func pixels() -> Data { editorPixels(in: app, at: sample, size: 32) }
            func expect(_ expected: Data) {
                expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
                waitForExpectations(timeout: 15)
            }
            func capture(_ state: String) {
                phoneCapture(in: app, id: phoneCaptureID("input", state), theme: theme, fixture: actions)
            }
            func stroke() {
                canvas.coordinate(withNormalizedOffset: CGVector(dx: (region.minX + region.width * 0.4 - bounds.minX) / bounds.width, dy: sample.y)).press(forDuration: 0,
                    thenDragTo: canvas.coordinate(withNormalizedOffset: CGVector(dx: (region.minX + region.width * 0.6 - bounds.minX) / bounds.width, dy: sample.y)),
                    withVelocity: .fast, thenHoldForDuration: 0)
            }
            func chooseFinger(_ mode: String) {
                phoneChooseFingerMode(mode, in: app)
                workspaceActivate(app.buttons["phone-capy"])
                XCTAssertTrue(app.descendants(matching: .any)["phone-menu"].firstMatch.waitForNonExistence(timeout: 10))
            }
            let blank = pixels()
            stroke()
            expectation(for: NSPredicate { _, _ in pixels() != blank }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            capture("input-auto-stroke")
            phoneAlertCommand("Edit", label: "Undo", in: app); expect(blank)
            chooseFinger("Off"); stroke()
            phoneAlertCommand("View", label: "Fit canvas", in: app)
            expect(blank); capture("input-off")
            chooseFinger("Draw"); stroke()
            expectation(for: NSPredicate { _, _ in pixels() != blank }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            let painted = pixels(); capture("touch-stroke")
            canvas.twoFingerTap(); expect(blank)
            canvas.tap(withNumberOfTaps: 1, numberOfTouches: 3); expect(painted)
            canvas.twoFingerTap(); expect(blank)
            let paper = editorPaper(in: app).frame
            canvas.pinch(withScale: 1.6, velocity: 1)
            expectation(for: NSPredicate { _, _ in self.editorPaper(in: app).frame != paper }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            phoneAlertCommand("View", label: "Reset rotation", in: app)
            phoneAlertCommand("View", label: "Fit canvas", in: app)
            expectation(for: NSPredicate { _, _ in self.editorPaper(in: app).frame == paper }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            expect(blank); capture("input-navigation")
            canvas.tap(withNumberOfTaps: 1, numberOfTouches: 4)
            XCTAssertTrue(app.buttons["phone-tool-settings"].waitForNonExistence(timeout: 10))
            capture("input-four-finger-zen")
            workspaceActivate(app.buttons["phone-capy"])
            XCTAssertTrue(app.buttons["phone-tool-settings"].waitForExistence(timeout: 10))
            expect(blank)
        }
    }

    @MainActor func testPhoneMenuVariantsLight() throws { try checkPhoneMenuVariants(theme: "light") }
    @MainActor func testPhoneMenuVariantsDark() throws { try checkPhoneMenuVariants(theme: "dark") }

    @MainActor private func checkPhoneMenuVariants(theme: String) throws {
        for orientation in phoneCaptureOrientations {
            requestedPhoneOrientation = orientation
            let app = try phoneCaptureApplication(theme: theme)
            defer { app.terminate() }
            let initial = try XCTUnwrap(app.launchEnvironment["CAPY_INITIAL_ACTIONS"]?.data(using: .utf8))
            let baseline = try XCTUnwrap(try JSONSerialization.jsonObject(with: initial) as? [[String: Any]])
            func capture(_ state: String, _ fixture: [[String: Any]] = []) {
                phoneCapture(in: app, id: phoneCaptureID("menu", state), theme: theme, fixture: baseline + fixture)
            }
            _ = phoneMenuRoot(in: app)
            let finger = app.buttons["phone-finger"]
            phoneChooseFingerMode("Off", in: app)
            workspaceActivate(app.buttons["phone-capy"]); capture("rest")
            _ = phoneMenuRoot(in: app)
            for swapped in [false, true] {
                if swapped { workspaceActivate(app.buttons["phone-swap"]) }
                let suffix = swapped ? "-swapped" : ""
                for (index, mode) in ["Auto", "Draw", "Move canvas", "Off"].enumerated() {
                    phoneChooseFingerMode(mode, in: app)
                    let key = ["auto", "on", "move_canvas", "off"][index]
                    let fixture: [[String: Any]] = [["type": "preferences", "action": ["type": "edit", "id": "finger_drawing", "value": index]]]
                    capture("menu-finger-" + key + suffix, fixture)
                    if mode == "Off" {
                        workspaceActivate(finger)
                        phoneWaitForMenuTitle("One-finger drag", in: app)
                        capture("menu-one-finger-drag" + suffix, fixture)
                        workspaceActivate(app.buttons["phone-menu-back"])
                        phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
                    }
                }
            }
            workspaceActivate(app.buttons["phone-swap"])
            for (id, state) in [("phone-swap", "menu-swap-checked")] {
                let button = app.buttons[id]
                XCTAssertFalse(button.isSelected); workspaceActivate(button)
                expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: button); waitForExpectations(timeout: 10)
                capture(state); workspaceActivate(button)
                expectation(for: NSPredicate(format: "selected == NO"), evaluatedWith: button); waitForExpectations(timeout: 10)
            }
            func transparency(_ value: Int) {
                workspaceActivate(app.buttons["phone-preferences"])
                XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 10))
                workspaceActivate(app.buttons["settings-page-appearance"])
                let option = app.buttons["preference-transparency-\(value)"]
                XCTAssertTrue(option.waitForExistence(timeout: 10)); workspaceActivate(option)
                expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: option); waitForExpectations(timeout: 10)
                workspaceActivate(app.buttons["settings-done"])
                XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 10)); _ = phoneMenuRoot(in: app)
            }
            for swapped in [false, true] {
                workspaceActivate(app.buttons["phone-capy"]); phoneSetSwapped(swapped, in: app); _ = phoneMenuRoot(in: app)
                for (value, material) in [(0, "opaque"), (2, "glass")] {
                    transparency(value)
                    capture("menu-root-" + (swapped ? "swapped" : "normal") + "-" + material,
                        [["type": "preferences", "action": ["type": "edit", "id": "transparency", "value": value]]])
                }
            }
            transparency(1); workspaceActivate(app.buttons["phone-capy"]); phoneSetSwapped(false, in: app)
            let storage = try XCTUnwrap(app.launchEnvironment["CAPY_STORAGE_DIR"])
            let original = try XCTUnwrap(phoneDrawingFixtures[storage])
            func drawings() {
                let scroll = phoneMenuRoot(in: app), button = app.buttons["phone-drawing"]
                revealEditorControl(button, in: scroll); workspaceActivate(button); phoneWaitForMenuTitle("Drawings", in: app)
            }
            let selected = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND selected == YES", "phone-menu-row-"))
            let closes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-close-"))
            drawings(); XCTAssertEqual(closes.count, 1); XCTAssertEqual(selected.count, 1)
            let first = selected.firstMatch.identifier
            workspaceActivate(app.buttons["phone-capy"])
            phoneAlertCommand("Layer", label: "New layer", in: app)
            createWhitePaperDrawing(width: "128", height: "96", in: app, open: {
                self.phoneAlertCommand("File", label: "New…", in: app)
                self.revealNewDrawingControl("new-drawing-name", in: app)
                self.replaceEditorText(app.textFields["new-drawing-name"], with: "Menu drawing", in: app)
                self.dismissEditorKeyboard(in: app); self.enterNewDrawingNumber("resolution", "300", in: app)
                self.chooseNewDrawingOption("new-drawing-space", 0, in: app); self.expectNewDrawingValue("new-drawing-space", "sRGB", in: app)
            })
            phoneAlertCommand("Image", label: "Canvas Size…", in: app)
            let size = app.staticTexts["canvas-size-message"]
            XCTAssertTrue(size.waitForExistence(timeout: 15)); XCTAssertEqual(editorText(size), "Current size: 128 × 96 px")
            workspaceActivate(app.buttons["canvas-size-cancel"]); XCTAssertTrue(size.waitForNonExistence(timeout: 10))
            drawings()
            expectation(for: NSPredicate { _, _ in closes.count == 2 && selected.count == 1 && selected.firstMatch.label.contains("Menu drawing") }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            XCTAssertTrue(selected.firstMatch.label.contains("Menu drawing")); XCTAssertNotEqual(selected.firstMatch.identifier, first)
            XCTAssertTrue(phoneMenuRow("New drawing…", in: app).exists)
            var proof = original
            proof["width"] = 128; proof["height"] = 96; proof["name"] = "Menu drawing"
            proof["actual_canvas_size"] = "Current size: 128 × 96 px"; proof["document_count"] = 2
            proof["selected_drawing"] = selected.firstMatch.label; phoneDrawingFixtures[storage] = proof
            workspaceActivate(app.buttons["phone-menu-back"]); capture("menu-root-two")
            workspaceActivate(app.buttons["phone-drawing"]); phoneWaitForMenuTitle("Drawings", in: app); capture("menu-drawings-two")
            let firstRow = app.buttons[first]; workspaceActivate(firstRow)
            XCTAssertTrue(app.buttons["phone-menu-back"].waitForNonExistence(timeout: 10))
            phoneAlertCommand("Image", label: "Canvas Size…", in: app)
            XCTAssertTrue(size.waitForExistence(timeout: 15)); XCTAssertEqual(editorText(size), "Current size: 1200 × 1600 px")
            workspaceActivate(app.buttons["canvas-size-cancel"]); XCTAssertTrue(size.waitForNonExistence(timeout: 10))
            drawings(); XCTAssertEqual(closes.count, 2); XCTAssertTrue(firstRow.isSelected)
            XCTAssertTrue(phoneMenuRow("New drawing…", in: app).exists)
            proof = original; proof["document_count"] = 2; proof["selected_drawing"] = firstRow.label; phoneDrawingFixtures[storage] = proof
            capture("menu-drawings-current")
            workspaceActivate(app.buttons[first.replacingOccurrences(of: "phone-menu-row-", with: "phone-menu-close-")])
            let discard = app.buttons["document-close-discard"]
            XCTAssertTrue(discard.waitForExistence(timeout: 10)); workspaceActivate(discard)
            XCTAssertTrue(discard.waitForNonExistence(timeout: 10))
            drawings(); XCTAssertEqual(closes.count, 1); XCTAssertTrue(selected.firstMatch.label.contains("Menu drawing"))
            workspaceActivate(app.buttons["phone-capy"])
        }
    }

    @MainActor var phoneCaptureOrientations: [UIDeviceOrientation] {
        requestedPhoneOrientation.map { [$0] } ?? [.portrait, .landscapeLeft, .landscapeRight]
    }

    @MainActor private func checkPhoneFrame(theme: String) throws {
        requestedPhoneOrientation = requestedPhoneOrientation ?? .portrait
        let app = try phoneFrameApplication(theme: theme)
        defer { app.terminate() }
        try visitPhoneFrameVariants(in: app, theme: theme)
    }

    @MainActor private func phoneFrameApplication(theme: String) throws -> XCUIApplication {
        let app = try phoneCaptureApplication(theme: theme)
        let settings = app.buttons["phone-tool-settings"]
        let drawer = app.descendants(matching: .any)["tool-drawer"].firstMatch
        let panel = app.descendants(matching: .any)["drawer-panel-tool_settings"].firstMatch
        for open in [true, false] {
            workspaceActivate(settings)
            let published = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                drawer.exists == open && (!open || panel.exists)
            }, object: app)
            guard XCTWaiter.wait(for: [published], timeout: 10) == .completed else {
                let screenshot = XCTAttachment(screenshot: app.screenshot()), tree = XCTAttachment(string: app.debugDescription)
                screenshot.name = "phone-tool-settings-blocked"; tree.name = "phone-tool-settings-hierarchy"
                screenshot.lifetime = .keepAlways; tree.lifetime = .keepAlways; add(screenshot); add(tree)
                XCTFail("Native phone Tool Settings input must \(open ? "open" : "close") its shared drawer"); return app
            }
        }
        return app
    }

    @MainActor private func checkPhoneLayout(theme: String) throws {
        for orientation in phoneCaptureOrientations {
            requestedPhoneOrientation = orientation
            let actions: [[String: Any]] = [["type": "set_theme", "theme": theme],
                ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:painter"]]]
            let app = try phoneFrameApplication(theme: theme)
            phoneCapture(in: app, id: phoneCaptureID("frame", "rest"), theme: theme, fixture: actions)
            try visitPhonePages(in: app, theme: theme, fixture: actions)
            app.terminate()
        }
    }

    @MainActor private func checkPhoneMenus(theme: String, orientation: UIDeviceOrientation, swapped: Bool, group: PhoneMenuGroup) throws {
        requestedPhoneOrientation = requestedPhoneOrientation ?? orientation
        let actions: [[String: Any]] = [["type": "set_theme", "theme": theme],
            ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:painter"]]]
        let app = try phoneFrameApplication(theme: theme)
        defer { app.terminate() }
        try visitPhoneMenu(in: app, theme: theme, fixture: actions, swapped: swapped, group: group)
    }

    @MainActor func phoneSetSwapped(_ swapped: Bool, in app: XCUIApplication) {
        let scroll = phoneMenuRoot(in: app), swap = app.buttons["phone-swap"]
        XCTAssertTrue(swap.waitForExistence(timeout: 10))
        if swap.isSelected != swapped { workspaceActivate(swap) }
        expectation(for: NSPredicate(format: "selected == %@", NSNumber(value: swapped)), evaluatedWith: swap)
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["phone-capy"])
        XCTAssertTrue(scroll.waitForNonExistence(timeout: 10))
    }

    @MainActor private func visitPhoneFrameVariants(in app: XCUIApplication, theme: String) throws {
        let initial = try XCTUnwrap(app.launchEnvironment["CAPY_INITIAL_ACTIONS"]?.data(using: .utf8))
        let baseline = try XCTUnwrap(try JSONSerialization.jsonObject(with: initial) as? [[String: Any]])
        let fixture = baseline + [["type": "set_brush_size", "value": 40], ["type": "set_color", "rgba": [0, 0, 0, 1]]]
        try phoneRelaunch(app, theme: theme, actions: fixture)
        phoneSetSwapped(false, in: app)
        XCTAssertEqual(phoneTools(in: app).count, 8)
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        _ = phoneMenuRoot(in: app)
        workspaceActivate(app.buttons["phone-zen"])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.buttons["phone-tool-settings"].exists)
        phoneCapture(in: app, id: phoneCaptureID("frame", "frame-hidden"), theme: theme, fixture: fixture)
        workspaceActivate(app.buttons["phone-capy"])
        XCTAssertTrue(app.buttons["phone-tool-settings"].waitForExistence(timeout: 10))
        XCTAssertFalse(menu.exists)
        phoneCapture(in: app, id: phoneCaptureID("frame", "frame-revealed"), theme: theme, fixture: fixture)
        for (value, state) in [(0, "frame-opaque"), (3, "frame-glass")] {
            let actions = fixture + [["type": "preferences", "action": ["type": "edit", "id": "transparency", "value": value]]]
            try phoneRelaunch(app, theme: theme, actions: actions)
            phoneSetSwapped(false, in: app)
            phoneCapture(in: app, id: phoneCaptureID("frame", state), theme: theme, fixture: actions)
        }
    }

    @MainActor private func visitPhoneMenu(in app: XCUIApplication, theme: String, fixture: [[String: Any]], swapped: Bool, group: PhoneMenuGroup) throws {
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        let scroll = app.descendants(matching: .any)["phone-menu-scroll"].firstMatch
        func open() {
            workspaceActivate(app.buttons["phone-capy"])
            phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
            XCTAssertTrue(scroll.waitForExistence(timeout: 10))
        }
        func reveal(_ element: XCUIElement) {
            revealEditorControl(element, in: scroll)
            XCTAssertTrue(element.isHittable)
        }
        open()
        phoneChooseFingerMode("Off", in: app)
        workspaceActivate(app.buttons["phone-capy"])
        if group != .search {
            var visited = Set<String>()
            if swapped { phoneSetSwapped(true, in: app) }
            open()
            let bounds = menu.frame
            func visit(_ id: PhoneCaptureID, labels: [String]) throws {
                guard let title = id.menu?["title"] as? String else { return XCTFail("A menu route needs its published title") }
                phoneWaitForMenuTitle(title, in: app)
                XCTAssertEqual(menu.frame.minX, bounds.minX, accuracy: 1)
                XCTAssertEqual(menu.frame.minY, bounds.minY, accuracy: 1)
                XCTAssertEqual(menu.frame.width, bounds.width, accuracy: 1)
                XCTAssertEqual(menu.frame.height, bounds.height, accuracy: 1)
                visited.insert(id.state)
                if !labels.isEmpty || group == .fileToLayer {
                    phoneCapture(in: app, id: id, theme: theme, fixture: fixture, extra: ["route_labels": labels, "swapped": swapped])
                }
                let children = menu.descendants(matching: .any).matching(identifier: "phone-menu-submenu").allElementsBoundByIndex
                    .map { $0.buttons.firstMatch }.filter { $0.exists && $0.isEnabled }.map { ($0.identifier, $0.label) }
                for (identifier, label) in children {
                    let row = app.buttons[identifier].firstMatch
                    guard let next = phoneMenuChildID(parent: id, row: row, swapped: swapped) else { return }
                    if labels.isEmpty {
                        guard let route = next.menu?["route"] as? [String: Any], let page = route["page"] as? [String: Any],
                            let kind = page["kind"] as? String else { return XCTFail("A root menu needs its published page") }
                        let included: Bool
                        switch kind {
                        case "application": included = group.menus.contains(page["menu"] as? String ?? "")
                        case "drawings", "workspaces": included = group == .selectToWorkspaces
                        default: return XCTFail("The menu groups must cover root page \(kind)")
                        }
                        if !included { continue }
                    }
                    reveal(row); workspaceActivate(row)
                    try visit(next, labels: labels + [label])
                    let back = app.buttons["phone-menu-back"]
                    reveal(back); workspaceActivate(back)
                    phoneWaitForMenuTitle(title, in: app)
                }
            }
            guard let root = phoneMenuID(route: ["path": [[Int]]()], swapped: swapped) else { return }
            try visit(root, labels: [])
            if !swapped && group == .selectToWorkspaces {
                let view = phoneMenuRow("View", in: app)
                guard let viewTitle = phoneMenuTitle(for: view) else { return }
                reveal(view); workspaceActivate(view)
                phoneWaitForMenuTitle(viewTitle, in: app)
                let back = app.buttons["phone-menu-back"], title = back.label
                let first = menu.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-row-")).firstMatch
                let before = first.frame
                let start = try XCTUnwrap(workspacePoint(scroll))
                start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: -scroll.frame.height / 3)), withVelocity: .slow, thenHoldForDuration: 0.1)
                XCTAssertTrue(menu.exists)
                XCTAssertEqual(back.label, title)
                XCTAssertTrue(!first.isHittable || first.frame.minY < before.minY)
                phoneCapture(in: app, id: phoneCaptureID("menu", "menu-touch-scrolled"), theme: theme, fixture: fixture)
                reveal(back); workspaceActivate(back)
                phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
                let file = phoneMenuRow("File", in: app)
                guard let fileTitle = phoneMenuTitle(for: file) else { return }
                reveal(file); workspaceActivate(file)
                phoneWaitForMenuTitle(fileTitle, in: app)
                let from = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.15, dy: 0.5))
                let short = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.5))
                from.press(forDuration: 0.05, thenDragTo: short, withVelocity: .slow, thenHoldForDuration: 0.4)
                phoneWaitForMenuTitle(fileTitle, in: app)
                phoneCapture(in: app, id: phoneCaptureID("menu", "menu-swipe-returned"), theme: theme, fixture: fixture)
                let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.5))
                from.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.4)
                phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
                XCTAssertFalse(back.exists)
                XCTAssertTrue(menu.exists)
                phoneCapture(in: app, id: phoneCaptureID("menu", "menu-swipe-completed"), theme: theme, fixture: fixture)
            }
            let bridge = app.descendants(matching: .any)["phone-menu-bridge"].firstMatch
            XCTAssertTrue(bridge.exists)
            XCTAssertGreaterThan(bridge.frame.width, 0)
            XCTAssertGreaterThan(bridge.frame.height, 0)
            workspacePoint(bridge)?.tap()
            for mode in ["Draw", "Off"] {
                phoneChooseFingerMode(mode, in: app)
                XCTAssertTrue(menu.exists)
            }
            if group == .fileToLayer {
                phoneCapture(in: app, id: phoneCaptureID("menu", "menu-strip-toggles" + (swapped ? "-swapped" : "")), theme: theme, fixture: fixture, extra: ["swapped": swapped])
            }
            workspaceActivate(app.buttons["phone-capy"])
            XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
            if group == .selectToWorkspaces {
                for kind in ["drawings", "workspaces"] {
                    XCTAssertTrue(visited.contains(phoneMenuID(route: ["path": [[Int]](), "page": ["kind": kind]], swapped: swapped)?.state ?? ""))
                }
            }
            for name in group.menus {
                XCTAssertTrue(visited.contains(phoneMenuID(route: ["path": [[Int]](), "page": ["kind": "application", "menu": name]], swapped: swapped)?.state ?? ""), "The shared menu must expose \(name)")
            }
            return
        }
        phoneAlertCommand("Layer", label: "New layer", in: app)
        open()
        workspaceActivate(app.buttons["phone-search"])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        let search = app.textFields["command-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: search)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["command-result-0"].label.hasPrefix("Undo"))
        phoneCapture(in: app, id: phoneCaptureID("pages", "command-search"), theme: theme, fixture: fixture)
        search.typeText("lay\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10), "Keyboard Search hides the keyboard")
        XCTAssertEqual(search.value as? String, "lay", "Keyboard Search keeps the query instead of running a command")
        XCTAssertTrue(app.buttons["command-result-0"].exists)
        workspaceActivate(app.buttons["command-search-close"])
        open(); workspaceActivate(app.buttons["phone-preferences"])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 10))
        let settingsSearch = app.textFields["settings-search"]
        replaceEditorText(settingsSearch, with: "brush", in: app)
        settingsSearch.typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10))
        XCTAssertEqual(settingsSearch.value as? String, "brush")
        XCTAssertTrue(app.buttons["settings-done"].isHittable)
        workspaceActivate(app.buttons["settings-done"])
        open(); workspaceActivate(app.buttons["phone-zen"])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.buttons["phone-tool-settings"].exists)
        phoneCapture(in: app, id: phoneCaptureID("frame", "hidden"), theme: theme, fixture: fixture)
        workspaceActivate(app.buttons["phone-capy"])
        XCTAssertTrue(app.buttons["phone-tool-settings"].waitForExistence(timeout: 10))
        XCTAssertFalse(menu.exists)
    }

    @MainActor private func visitPhonePages(in app: XCUIApplication, theme: String, fixture: [[String: Any]]) throws {
        let bars = ["phone-bar-0", "phone-bar-1"].map { app.descendants(matching: .any)[$0].firstMatch.frame }
        let viewport = app.windows.firstMatch.frame
        let orientation = XCUIDevice.shared.orientation
        for bar in bars {
            XCTAssertTrue(viewport.contains(bar))
            if orientation == .portrait {
                XCTAssertEqual(bar.minX - viewport.minX, 8, accuracy: 1)
                XCTAssertEqual(viewport.maxX - bar.maxX, 8, accuracy: 1)
            } else {
                XCTAssertEqual(bar.minY - viewport.minY, 8, accuracy: 1)
                XCTAssertEqual(viewport.maxY - bar.maxY, 8, accuracy: 1)
            }
        }
        if orientation == .portrait {
            XCTAssertEqual(viewport.maxY - bars.map(\.maxY).max()!, 8, accuracy: 1)
            XCTAssertGreaterThanOrEqual(bars.map(\.minY).min()! - viewport.minY, 8)
        } else if orientation == .landscapeLeft {
            XCTAssertEqual(viewport.maxX - bars.map(\.maxX).max()!, 8, accuracy: 1)
            XCTAssertGreaterThanOrEqual(bars.map(\.minX).min()! - viewport.minX, 8)
        } else {
            XCTAssertEqual(bars.map(\.minX).min()! - viewport.minX, 8, accuracy: 1)
            XCTAssertGreaterThanOrEqual(viewport.maxX - bars.map(\.maxX).max()!, 8)
        }
        func unchangedBars() {
            for (index, before) in bars.enumerated() {
                let bar = app.descendants(matching: .any)["phone-bar-" + String(index)].firstMatch
                XCTAssertTrue(bar.exists)
                XCTAssertEqual(bar.frame, before)
            }
        }
        _ = phoneMenuRoot(in: app); unchangedBars()
        workspaceActivate(app.buttons["phone-capy"])
        XCTAssertTrue(app.descendants(matching: .any)["phone-menu"].firstMatch.waitForNonExistence(timeout: 10))
        unchangedBars()
        workspaceActivate(app.buttons["phone-tool-settings"])
        let value = app.buttons["number-value-tool-size"]
        XCTAssertTrue(value.waitForExistence(timeout: 10))
        let valueScroll = app.scrollViews.containing(.button, identifier: "number-value-tool-size").allElementsBoundByIndex.last
        if let valueScroll { revealEditorControl(value, in: valueScroll) }
        workspaceActivate(value)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        unchangedBars()
        app.textFields["number-entry-tool-size"].typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10))
        unchangedBars()
        let color = try XCTUnwrap(phoneObservedTools(in: app)?.first(where: { ($0["control"] as? [String: Any])?["kind"] as? String == "color" })?["id"] as? Int)
        workspaceActivate(app.buttons["phone-tool-\(color)"])
        for id in ["drawer-panel-color", "drawer-panel-palettes"] {
            XCTAssertTrue(app.descendants(matching: .any)[id].firstMatch.waitForExistence(timeout: 10), "Tool Settings retargets to Color with \(id)")
        }
        workspaceActivate(app.buttons["phone-tool-settings"])
        XCTAssertTrue(app.descendants(matching: .any)["drawer-panel-tool_settings"].firstMatch.waitForExistence(timeout: 10))
        workspaceActivate(app.buttons["phone-tool-settings"])
        XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForNonExistence(timeout: 10))
        phoneAlertCommand("File", label: "New…", in: app)
        XCTAssertTrue(app.buttons["new-drawing-create"].waitForExistence(timeout: 10))
        for action in ["new-drawing-cancel", "new-drawing-create"].map({ app.buttons[$0] }) {
            XCTAssertTrue(action.isHittable); XCTAssertTrue(viewport.contains(action.frame), "\(action.identifier) \(action.frame) in \(viewport)")
        }
        XCTAssertTrue(app.staticTexts["new-drawing-title"].exists)
        phoneCapture(in: app, id: phoneCaptureID("pages", "new-drawing"), theme: theme, fixture: fixture)
        revealNewDrawingControl("new-drawing-name", in: app)
        let name = app.textFields["new-drawing-name"]
        replaceEditorText(name, with: "Phone Return", in: app); name.typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10))
        XCTAssertEqual(name.value as? String, "Phone Return")
        XCTAssertTrue(app.buttons["new-drawing-create"].exists)
        revealNewDrawingControl("number-entry-new-drawing-resolution", in: app)
        let resolution = newDrawingNumber("resolution", in: app)
        replaceEditorText(resolution, with: "1 / 0", in: app); resolution.typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.exists)
        XCTAssertTrue(app.buttons["new-drawing-create"].exists)
        XCTAssertEqual(resolution.value as? String, "1 / 0")
        expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: resolution)
        waitForExpectations(timeout: 10)
        replaceEditorText(resolution, with: "300", in: app); resolution.typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10))
        XCTAssertEqual(resolution.value as? String, "300")
        XCTAssertTrue(app.buttons["new-drawing-create"].exists)
        revealNewDrawingControl("new-drawing-presets-button", in: app)
        workspaceActivate(app.buttons["new-drawing-presets-button"])
        XCTAssertTrue(app.buttons["new-drawing-browser-back"].waitForExistence(timeout: 10))
        let search = app.textFields["new-drawing-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        XCTAssertFalse(app.textFields.matching(NSPredicate(format: "identifier == %@ AND hasKeyboardFocus == YES", search.identifier)).firstMatch.exists)
        workspaceActivate(search); search.typeText("Phone audit")
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["new-drawing-browser-back"].isHittable)
        XCTAssertLessThanOrEqual(app.buttons["new-drawing-browser-back"].frame.maxY, app.keyboards.firstMatch.frame.minY + 1)
        search.typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "Phone audit"), evaluatedWith: search)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["new-drawing-browser-back"].exists)
        XCTAssertTrue(app.buttons["new-drawing-create"].exists)
        workspaceActivate(app.buttons["new-drawing-browser-back"])
        expectation(for: NSPredicate { _, _ in self.editorTextMatches(search, "") }, evaluatedWith: search)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["new-drawing-browser-back"].exists)
        workspaceActivate(app.buttons["new-drawing-browser-back"])
        XCTAssertTrue(app.buttons["new-drawing-browser-back"].waitForNonExistence(timeout: 10))
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.buttons["new-drawing-create"].waitForExistence(timeout: 10))
        workspaceActivate(app.buttons["new-drawing-cancel"])
        XCTAssertTrue(app.buttons["new-drawing-create"].waitForNonExistence(timeout: 10))
        unchangedBars()
        for kind in ["canvas", "image"] {
            phoneAlertCommand("Image", label: kind == "canvas" ? "Canvas Size…" : "Image Size…", in: app)
            XCTAssertTrue(app.buttons[kind + "-size-apply"].waitForExistence(timeout: 10))
            XCTAssertFalse(app.buttons[kind + "-size-apply"].isEnabled)
            XCTAssertTrue(app.buttons[kind + "-size-cancel"].isHittable)
            XCTAssertTrue(app.staticTexts[kind + "-size-title"].exists)
            phoneCapture(in: app, id: phoneCaptureID("pages", kind + "-size"), theme: theme, fixture: fixture)
            workspaceActivate(app.buttons[kind + "-size-cancel"])
            XCTAssertTrue(app.buttons[kind + "-size-apply"].waitForNonExistence(timeout: 10))
            unchangedBars()
        }
    }
}

extension EditorLaunchTests {
    @MainActor func phoneLaunch(_ app: XCUIApplication) {
        let requested = requestedPhoneOrientation ?? XCUIDevice.shared.orientation
        guard requested.isPortrait || requested.isLandscape else {
            return XCTFail("Phone captures require an explicit portrait or landscape orientation")
        }
        XCUIDevice.shared.orientation = requested
        app.launch()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 15))
        let window = app.windows.firstMatch
        let oriented = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard window.exists else { return false }
            let bounds = window.frame
            return bounds.width > 0 && bounds.height > 0
                && (requested.isLandscape ? bounds.width > bounds.height : bounds.height > bounds.width)
        }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [oriented], timeout: 15), .completed,
            "The foreground phone window must reach its requested orientation before input or capture")
    }

    @MainActor func phoneRelaunch(_ app: XCUIApplication, theme: String, actions: [[String: Any]]) throws {
        if app.state != .notRunning {
            let persistence = app.staticTexts["persistence-status"], recovery = app.staticTexts["recovery-status"]
            expectation(for: NSPredicate(format: "label == %@", "Saved"), evaluatedWith: persistence)
            expectation(for: NSPredicate(format: "label == %@", "Recovery ready"), evaluatedWith: recovery)
            waitForExpectations(timeout: 60)
            app.terminate()
        }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(decoding: try JSONSerialization.data(withJSONObject: actions), as: UTF8.self)
        phoneLaunch(app)
        let workspace = actions.reversed().compactMap { action -> String? in
            guard action["type"] as? String == "workspace_manager", let command = action["command"] as? [String: Any],
                command["type"] as? String == "switch" else { return nil }
            return command["id"] as? String
        }.first
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let persistence = app.staticTexts["persistence-status"], recovery = app.staticTexts["recovery-status"]
        let startup = app.staticTexts["persistence-startup-ready"], currentWorkspace = app.staticTexts["persistence-workspace"]
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            guard canvas.exists, persistence.exists, recovery.exists, startup.exists, currentWorkspace.exists else { return false }
            return canvas.value as? String == "Canvas ready" && startup.value as? String == "true"
                && persistence.value as? String == theme && currentWorkspace.value as? String == workspace
                && persistence.label == "Saved" && recovery.label == "Recovery ready"
        }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 60), .completed,
            "The shared canvas, startup actions, shaders, theme, workspace and durable storage must be ready before phone input")
        XCTAssertTrue(app.buttons["phone-capy"].waitForExistence(timeout: 15))
    }

    @MainActor func phoneCaptureApplication(theme: String, actions: [[String: Any]] = [], ready: String? = nil) throws -> XCUIApplication {
        let app = editorCaptureApplication()
        app.launchArguments += ["-AppleLanguages", "(en)", "-AppleInterfaceStyle", theme == "dark" ? "Dark" : "Light"]
        let baseline: [[String: Any]] = [["type": "set_theme", "theme": theme],
            ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:painter"]],
            ["type": "invoke", "command": "sculpt"], ["type": "select_brush_set", "group": "blend"],
            ["type": "select_brush", "id": 2]]
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        if ready == nil { try phoneRelaunch(app, theme: theme, actions: baseline) }
        else {
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(decoding: try JSONSerialization.data(withJSONObject: baseline + actions), as: UTF8.self)
            phoneLaunch(app)
        }
        guard min(app.frame.width, app.frame.height) < 480 else {
            XCTFail("Phone captures require an iPhone or a narrow iPad window")
            app.terminate(); return app
        }
        if let ready { XCTAssertTrue(app.descendants(matching: .any)[ready].firstMatch.waitForExistence(timeout: 60)) }
        else {
            try phoneCreateCanonicalDrawing(in: app)
            if !actions.isEmpty { try phoneRelaunch(app, theme: theme, actions: baseline + actions) }
        }
        return app
    }
    @MainActor private func phoneCreateCanonicalDrawing(in app: XCUIApplication) throws {
        createWhitePaperDrawing(width: "1200", height: "1600", in: app, open: {
            self.phoneAlertCommand("File", label: "New…", in: app)
            XCTAssertTrue(app.buttons["new-drawing-create"].waitForExistence(timeout: 60))
            self.revealNewDrawingControl("new-drawing-name", in: app)
            self.replaceEditorText(app.textFields["new-drawing-name"], with: "Phone drawing", in: app)
            XCTAssertEqual(app.textFields["new-drawing-name"].value as? String, "Phone drawing")
            self.dismissEditorKeyboard(in: app)
            self.enterNewDrawingNumber("resolution", "300", in: app)
            XCTAssertEqual(self.newDrawingNumber("resolution", in: app).value as? String, "300")
            self.chooseNewDrawingOption("new-drawing-space", 0, in: app)
            self.expectNewDrawingValue("new-drawing-space", "sRGB", in: app)
        })
        phoneAlertCommand("Image", label: "Canvas Size…", in: app)
        let size = app.staticTexts["canvas-size-message"]
        XCTAssertTrue(size.waitForExistence(timeout: 15))
        let actualSize = editorText(size)
        XCTAssertEqual(actualSize, "Current size: 1200 × 1600 px")
        workspaceActivate(app.buttons["canvas-size-cancel"])
        XCTAssertTrue(size.waitForNonExistence(timeout: 10))
        let scroll = phoneMenuRoot(in: app)
        let drawings = app.buttons["phone-drawing"]
        XCTAssertTrue(drawings.waitForExistence(timeout: 10))
        revealEditorControl(drawings, in: scroll); workspaceActivate(drawings)
        let selected = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND selected == YES", "phone-menu-row-")).firstMatch
        XCTAssertTrue(selected.waitForExistence(timeout: 10))
        XCTAssertTrue(selected.label.contains("Phone drawing"))
        let canonicalClose = selected.identifier.replacingOccurrences(of: "phone-menu-row-", with: "phone-menu-close-")
        let closes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-close-"))
        XCTAssertEqual(closes.count, 2)
        let startup = closes.matching(NSPredicate(format: "identifier != %@", canonicalClose)).firstMatch
        XCTAssertTrue(startup.exists)
        revealEditorControl(startup, in: scroll); workspaceActivate(startup)
        XCTAssertTrue(scroll.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.buttons["document-close-discard"].exists, "Only the untouched private startup drawing may close")
        let remaining = phoneMenuRoot(in: app)
        revealEditorControl(drawings, in: remaining); workspaceActivate(drawings)
        let back = app.buttons["phone-menu-back"]
        let closed = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            back.exists && back.label == "Drawings" && closes.count == 1
        }, object: app)
        guard XCTWaiter.wait(for: [closed], timeout: 15) == .completed else {
            let screenshot = XCTAttachment(screenshot: app.screenshot()), tree = XCTAttachment(string: app.debugDescription)
            screenshot.name = "canonical-startup-close-blocked"; tree.name = "canonical-startup-close-hierarchy"
            screenshot.lifetime = .keepAlways; tree.lifetime = .keepAlways; add(screenshot); add(tree)
            return XCTFail("The Drawings page must publish the single remaining canonical drawing")
        }
        XCTAssertEqual(closes.count, 1)
        XCTAssertTrue(selected.isSelected)
        let title = selected.label
        workspaceActivate(app.buttons["phone-capy"])
        let storage = try XCTUnwrap(app.launchEnvironment["CAPY_STORAGE_DIR"])
        let proof: [String: Any] = ["status": "UI-created", "width": 1200, "height": 1600, "resolution": 300,
            "space": "Srgb", "depth": "U8", "paper": "white", "relief": 0, "name": "Phone drawing",
            "actual_canvas_size": actualSize, "selected_drawing": title, "document_count": 1,
            "future_new_drawing_defaults": "UI-created canonical recipe"]
        phoneDrawingFixtures[storage] = proof
        addTeardownBlock { MainActor.assumeIsolated { _ = phoneDrawingFixtures.removeValue(forKey: storage) } }
        let attachment = XCTAttachment(data: try JSONSerialization.data(withJSONObject: proof, options: [.sortedKeys]), uniformTypeIdentifier: "public.json")
        attachment.name = "phone-canonical-drawing-fixture"; attachment.lifetime = .keepAlways; add(attachment)
    }
}
