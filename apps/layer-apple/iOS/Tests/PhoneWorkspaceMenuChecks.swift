import XCTest
import UIKit

private enum PhoneWorkspaceMenuPhase { case all, overflow, drawings }

extension EditorLaunchTests {
    @MainActor func testPhoneWorkspaceMenuPortraitLight() throws { try checkPhoneWorkspaceMenu(theme: "light", orientation: .portrait) }
    @MainActor func testPhoneWorkspaceMenuPortraitDark() throws { try checkPhoneWorkspaceMenu(theme: "dark", orientation: .portrait) }
    @MainActor func testPhoneWorkspaceMenuLandscapeLight() throws { try checkPhoneWorkspaceMenu(theme: "light", orientation: .landscapeLeft) }
    @MainActor func testPhoneWorkspaceMenuLandscapeDark() throws { try checkPhoneWorkspaceMenu(theme: "dark", orientation: .landscapeLeft) }
    @MainActor func testPhoneWorkspaceMenuOverflowPortraitLight() throws { try checkPhoneWorkspaceMenu(theme: "light", orientation: .portrait, phase: .overflow) }
    @MainActor func testPhoneWorkspaceMenuDrawingsPortraitLight() throws { try checkPhoneWorkspaceMenu(theme: "light", orientation: .portrait, phase: .drawings) }

    @MainActor private func checkPhoneWorkspaceMenu(theme: String, orientation: UIDeviceOrientation, phase: PhoneWorkspaceMenuPhase = .all) throws {
        continueAfterFailure = false
        requestedPhoneOrientation = orientation
        let app = try phoneCaptureApplication(theme: theme)
        defer { app.terminate() }
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        let list = app.descendants(matching: .any)["phone-menu-scroll"].firstMatch
        let row = app.buttons["phone-menu-switcher-row"]
        let well = app.descendants(matching: .any)["phone-menu-switcher"].firstMatch
        let options = app.buttons["phone-menu-switcher-options"]
        let drawing = app.buttons["phone-drawing"]
        let back = app.buttons["phone-menu-back"]
        let pills = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-switch-"))
        let active = app.staticTexts["persistence-workspace"]
        let window = workspaceViewport(in: app)
        let rootTitle = app.buttons["phone-capy"].label
        func wait(_ message: String, _ condition: @escaping () -> Bool) {
            let event = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [event], timeout: 15), .completed, message)
        }
        func point(_ x: CGFloat, _ y: CGFloat) -> XCUICoordinate {
            XCTAssertTrue(window.frame.contains(CGPoint(x: x, y: y)))
            return window.coordinate(withNormalizedOffset: .zero)
                .withOffset(CGVector(dx: x - window.frame.minX, dy: y - window.frame.minY))
        }
        func frame(_ rect: CGRect) -> [CGFloat] { [rect.minX, rect.minY, rect.width, rect.height] }
        func capture(_ name: String) throws {
            let nodes = [menu, list, row, well, options, drawing, back] + pills.allElementsBoundByIndex
                + app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-row-")).allElementsBoundByIndex
                + app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-close-")).allElementsBoundByIndex
                + [app.buttons["phone-preferences"], app.buttons["phone-search"], app.buttons["phone-finger"],
                   app.buttons["phone-zen"], app.buttons["phone-swap"]] + phoneTools(in: app)
            let records = nodes.filter(\.exists).map { element -> [String: Any] in
                ["identifier": element.identifier, "label": element.label, "frame": frame(element.frame),
                 "selected": element.isSelected, "hittable": element.isHittable]
            }
            let shot = window.screenshot()
            let image = editorImage(from: shot)
            let fields: [String: Any] = [
                "theme": theme, "workspace": try XCTUnwrap(active.value as? String),
                "viewport": frame(window.frame), "pixel_size": [image.width, image.height],
                "dpr": CGFloat(image.width) / window.frame.width, "elements": records,
                "phone_tools": try XCTUnwrap(phoneObservedTools(in: app))
            ]
            let png = XCTAttachment(screenshot: shot)
            png.name = "phone-switcher-" + name; png.lifetime = .keepAlways; add(png)
            let geometry = XCTAttachment(data: try JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]),
                uniformTypeIdentifier: "public.json")
            geometry.name = "phone-switcher-" + name + "-geometry"; geometry.lifetime = .keepAlways; add(geometry)
        }
        func expectRoot() {
            phoneWaitForMenuTitle(rootTitle, in: app)
            XCTAssertTrue(row.exists && well.exists && options.exists && drawing.exists)
            XCTAssertFalse(back.exists)
            XCTAssertEqual(row.frame.height, drawing.frame.height, accuracy: 1)
            XCTAssertGreaterThan(well.frame.height, 0)
            XCTAssertEqual(drawing.frame.minY - row.frame.maxY, 4, accuracy: 1)
            if orientation != .portrait { XCTAssertGreaterThanOrEqual(list.frame.width, 319) }
        }
        func captureRoot(_ name: String) throws {
            expectRoot()
            try capture(name)
        }
        func closeMenu() {
            workspaceActivate(app.buttons["phone-capy"])
            XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        }
        func openMenu() { _ = phoneMenuRoot(in: app); expectRoot() }
        func choose(_ label: String, id: String) {
            let pill = pills.matching(NSPredicate(format: "label == %@", label)).firstMatch
            XCTAssertTrue(pill.waitForExistence(timeout: 10))
            let midpoint = CGPoint(x: pill.frame.midX, y: pill.frame.midY)
            XCTAssertTrue(well.frame.contains(midpoint) && list.frame.contains(midpoint) && window.frame.contains(midpoint))
            XCTAssertFalse(pill.isSelected)
            point(midpoint.x, midpoint.y).press(forDuration: 0.1)
            wait("The physical pill tap must switch workspace and keep the menu at root") {
                menu.exists && menu.label == rootTitle && active.value as? String == id
                    && pills.matching(NSPredicate(format: "label == %@ AND selected == YES", label)).firstMatch.exists
            }
            expectRoot()
        }
        func expectedControls(_ workspace: String) throws {
            let controls: [String: [[String: String]]] = [
                "builtin:workspace:painter": [
                    ["kind":"command","command":"pen"], ["kind":"command","command":"pencil"], ["kind":"tool_slot","slot":"paint"],
                    ["kind":"command","command":"sculpt"], ["kind":"command","command":"eraser"], ["kind":"tool_slot","slot":"bucket"],
                    ["kind":"panel","panel":"layers"], ["kind":"color"]],
                "builtin:workspace:illustrator": [
                    ["kind":"command","command":"select"], ["kind":"tool_slot","slot":"operation"], ["kind":"command","command":"drawing_brush"],
                    ["kind":"command","command":"sculpt"], ["kind":"command","command":"eraser"], ["kind":"tool_slot","slot":"bucket"],
                    ["kind":"panel","panel":"layers"], ["kind":"color"]],
                "builtin:workspace:photographer": [
                    ["kind":"panel","panel":"adjustments"], ["kind":"command","command":"select"], ["kind":"tool_slot","slot":"operation"],
                    ["kind":"command","command":"drawing_brush"], ["kind":"tool_slot","slot":"photo_retouch"], ["kind":"command","command":"eraser"],
                    ["kind":"panel","panel":"layers"], ["kind":"color"]]
            ]
            let tools = try XCTUnwrap(phoneObservedTools(in: app))
            let actual = try tools.map { try XCTUnwrap($0["control"] as? [String: String]) }
            XCTAssertEqual(actual, try XCTUnwrap(controls[workspace]))
            XCTAssertEqual(phoneTools(in: app).count, 8)
        }

        if phase == .all {
            try expectedControls("builtin:workspace:painter")
            try capture("sketch-toolbar")
            openMenu()
            try captureRoot("root-sketch")
            choose("Paint", id: "builtin:workspace:illustrator")
            try expectedControls("builtin:workspace:illustrator")
            try captureRoot("pill-paint-menu-stays-open")
            closeMenu()
            try capture("paint-toolbar")
            openMenu()
            choose("Photo", id: "builtin:workspace:photographer")
            try expectedControls("builtin:workspace:photographer")
            try captureRoot("pill-photo-menu-stays-open")
            closeMenu()
            try capture("photo-toolbar")
            openMenu()
            choose("Sketch", id: "builtin:workspace:painter")

            let padding = CGPoint(x: options.frame.maxX + 2.5, y: options.frame.midY)
            XCTAssertTrue(well.frame.contains(padding) && row.frame.contains(padding))
            XCTAssertFalse(options.frame.contains(padding))
            XCTAssertFalse(pills.allElementsBoundByIndex.contains { $0.frame.contains(padding) })
            point(padding.x, padding.y).tap()
            expectRoot()
            XCTAssertEqual(active.value as? String, "builtin:workspace:painter")
            try captureRoot("well-padding-does-nothing")

            for hit in ["options", "caret", "background"] {
                expectRoot()
                let target: CGPoint
                switch hit {
                case "options": target = CGPoint(x: options.frame.midX, y: options.frame.midY)
                case "caret": target = CGPoint(x: row.frame.maxX - 13, y: row.frame.midY)
                default: target = CGPoint(x: row.frame.minX + 5, y: row.frame.midY)
                }
                XCTAssertTrue(row.frame.contains(target))
                if hit != "options" { XCTAssertFalse(well.frame.contains(target)) }
                point(target.x, target.y).tap()
                phoneWaitForMenuTitle("Workspaces", in: app)
                XCTAssertTrue(back.exists)
                try capture(hit + "-workspaces")
                point(back.frame.midX, back.frame.midY).tap()
                phoneWaitForMenuTitle("Window", in: app)
                point(back.frame.midX, back.frame.midY).tap()
                expectRoot()
            }

            closeMenu()
        }

        if phase != .overflow {
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
            openMenu()
            try captureRoot("root-two-drawings")
            point(drawing.frame.midX, drawing.frame.midY).tap()
            phoneWaitForMenuTitle("Drawings", in: app)
            let closes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-close-"))
            let selectedDrawing = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND selected == YES", "phone-menu-row-"))
            wait("The new 128 × 96 drawing and original must appear in Drawings") {
                closes.count == 2 && selectedDrawing.count == 1 && selectedDrawing.firstMatch.label.contains("Menu drawing")
            }
            let rows = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-row-"))
            XCTAssertTrue(rows.allElementsBoundByIndex.contains { $0.label.hasPrefix("•") })
            try capture("drawings-two")
            closeMenu()
        }

        if phase == .drawings { return }
        phoneAlertCommand("Window", label: "Manage Workspaces…", in: app)
        let manager = app.descendants(matching: .any)["workspace-library-manager"].firstMatch
        XCTAssertTrue(manager.waitForExistence(timeout: 15))
        let create = app.buttons["workspace-action-new"]
        var lastName = ""
        for index in 0..<5 {
            workspaceActivate(create)
            let name = "Overflow Workspace " + String(index + 1)
            let field = app.textFields["workspace-form-name"]
            XCTAssertTrue(field.waitForExistence(timeout: 10))
            replaceEditorText(field, with: name, in: app)
            let confirm = app.buttons["workspace-form-confirm"]
            wait("Confirm the named workspace") { confirm.exists && confirm.isEnabled }
            workspaceActivate(confirm)
            XCTAssertTrue(field.waitForNonExistence(timeout: 15))
            XCTAssertTrue(manager.exists, "Creating a workspace keeps the manager open")
            let created = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                "workspace-select-", name)).firstMatch
            XCTAssertTrue(created.waitForExistence(timeout: 10))
            let id = String(created.identifier.dropFirst("workspace-select-".count))
            let pin = app.buttons["workspace-pin-" + id]
            let managerList = app.descendants(matching: .any)["workspace-manager-rows"].firstMatch
            revealEditorControl(pin, in: managerList)
            if !pin.isSelected { workspaceActivate(pin) }
            wait("The owned workspace must be pinned") { pin.isSelected }
            XCTAssertTrue(created.isSelected)
            lastName = name
        }
        workspaceActivate(app.buttons["workspace-manager-close"])
        XCTAssertTrue(manager.waitForNonExistence(timeout: 15))
        openMenu()
        XCTAssertGreaterThan(pills.count, 3)
        let selected = pills.matching(NSPredicate(format: "label == %@ AND selected == YES", lastName)).firstMatch
        XCTAssertTrue(selected.waitForExistence(timeout: 10))
        try captureRoot("overflow-selected-visible")
        let selectedMidpoint = CGPoint(x: selected.frame.midX, y: selected.frame.midY)
        XCTAssertTrue(well.frame.contains(selectedMidpoint) && list.frame.contains(selectedMidpoint)
            && window.frame.contains(selectedMidpoint) && selectedMidpoint.x < options.frame.minX,
            "Opening the overflow switcher must reveal its selected pill")
        let beforeX = selected.frame.minX
        let start = point(selected.frame.midX, selected.frame.midY)
        let endX = min(options.frame.minX - 4, selected.frame.midX + 100)
        XCTAssertGreaterThan(endX - selected.frame.midX, 15)
        let end = point(endX, selected.frame.midY)
        start.press(forDuration: 0.05, thenDragTo: end)
        wait("Horizontal touch scrolling must move the pinned workspace pills") {
            selected.exists && abs(selected.frame.minX - beforeX) > 1
        }
        expectRoot()
        try captureRoot("overflow-scrolled")
        closeMenu()
    }
}
