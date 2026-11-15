import XCTest
import UIKit

extension EditorLaunchTests {
    @MainActor func testPhoneAlertsLight() throws { try checkPhoneAlerts(theme: "light") }
    @MainActor func testPhoneAlertsDark() throws { try checkPhoneAlerts(theme: "dark") }
    @MainActor func testPhoneRecoveryAlertLight() throws { try checkPhoneRecoveryAlert(theme: "light") }
    @MainActor func testPhoneRecoveryAlertDark() throws { try checkPhoneRecoveryAlert(theme: "dark") }
    @MainActor func testPhonePackageAlertsLight() async throws { try await checkPhonePackageAlerts(theme: "light") }
    @MainActor func testPhonePackageAlertsDark() async throws { try await checkPhonePackageAlerts(theme: "dark") }

    @MainActor private func checkPhoneAlerts(theme: String) throws {
        let fixture: [[String: Any]] = [["type": "invoke", "command": "add_layer"]]
        let app = try phoneCaptureApplication(theme: theme, actions: fixture)
        defer { app.terminate() }
        phoneAlertCommand("File", label: "Close", in: app)
        let cancel = app.buttons["document-close-cancel"]
        XCTAssertTrue(cancel.waitForExistence(timeout: 15))
        for identifier in ["document-close-save", "document-close-discard", "document-close-cancel"] {
            XCTAssertTrue(app.buttons[identifier].isHittable, identifier)
        }
        let responses = ["document-close-save", "document-close-discard", "document-close-cancel"].map { app.buttons[$0].frame }
        let title = app.staticTexts["editor-alert-title"].frame
        for response in responses {
            XCTAssertTrue(app.windows.firstMatch.frame.contains(response))
            XCTAssertGreaterThanOrEqual(response.minY, title.maxY)
        }
        let stacked = responses[1].minY >= responses[0].maxY - 1
        for pair in zip(responses, responses.dropFirst()) {
            if stacked {
                XCTAssertGreaterThanOrEqual(pair.1.minY, pair.0.maxY - 1)
                XCTAssertEqual(pair.0.width, pair.1.width, accuracy: 1)
            } else { XCTAssertEqual(pair.0.midY, pair.1.midY, accuracy: 1) }
        }
        phoneCapture(in: app, id: phoneCaptureID("pages", "unsaved-changes"), theme: theme, fixture: fixture)
        workspaceActivate(cancel)
        XCTAssertTrue(cancel.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["phone-capy"])
        let drawings = app.buttons["phone-drawing"]
        revealEditorControl(drawings, in: app.descendants(matching: .any)["phone-menu-scroll"].firstMatch)
        workspaceActivate(drawings)
        XCTAssertTrue(app.buttons["phone-menu-back"].waitForExistence(timeout: 10))
    }

    @MainActor private func checkPhoneRecoveryAlert(theme: String) throws {
        let key = "CAPY_UI_MALFORMED_STORAGE_" + theme.uppercased()
        guard let storage = ProcessInfo.processInfo.environment[key] else {
            return XCTFail("Stage the existing private malformed recovery fixture through " + key)
        }
        guard storage.hasPrefix("capy-test-malformed-") else {
            XCTFail("Recovery capture requires the existing private malformed fixture namespace"); return
        }
        let app = try phoneCaptureApplication(theme: theme)
        defer { app.terminate() }
        app.terminate()
        app.launchEnvironment["CAPY_STORAGE_DIR"] = storage
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        phoneLaunch(app)
        let retry = app.buttons["recovery-retry"], later = app.buttons["recovery-later"]
        XCTAssertTrue(retry.waitForExistence(timeout: 60))
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: retry)
        waitForExpectations(timeout: 60)
        XCTAssertTrue(later.isHittable)
        let discards = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "recovery-discard-"))
        phoneCapture(in: app, id: phoneCaptureID("pages", "failed-drawing-recovery"), theme: theme, fixture: [],
            extra: ["storage_fixture": key, "discard_ids": discards.allElementsBoundByIndex.map(\.identifier)])
        workspaceActivate(later)
        XCTAssertTrue(later.waitForNonExistence(timeout: 10))
        if app.buttons["storage-ok"].exists {
            workspaceActivate(app.buttons["storage-ok"])
        }
    }

    @MainActor private func checkPhonePackageAlerts(theme: String) async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let token = environment["CAPY_FILE_TEST_TOKEN"], UUID(uuidString: token) != nil,
            let provider = environment["CAPY_UI_PACKAGE_PROVIDER"] else {
            return XCTFail("Stage the existing isolated Files-provider package fixture")
        }
        guard provider == "Capy Test Files " + token else { XCTFail("Private Files-provider identity mismatch"); return }
        let folder = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Capy Files " + token, isDirectory: true)
        let unsupported = folder.appendingPathComponent("Unsupported.capy")
        let invalid = folder.appendingPathComponent("PhoneAuditInvalid-" + UUID().uuidString + ".capy")
        try await Task.detached {
            let directory = try folder.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
            let source = try unsupported.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard directory.isDirectory == true, directory.isSymbolicLink != true,
                source.isRegularFile == true, source.isSymbolicLink != true else { throw CocoaError(.fileReadCorruptFile) }
            try Data("Not a Capy package".utf8).write(to: invalid, options: .atomic)
        }.value
        addTeardownBlock { try await Task.detached { try FileManager.default.removeItem(at: invalid) }.value }
        let app = try phoneCaptureApplication(theme: theme)
        defer { app.terminate() }
        phoneOpenProviderFile(invalid.lastPathComponent, token: token, provider: provider, in: app)
        let ok = app.buttons["file-error-ok"]
        XCTAssertTrue(ok.waitForExistence(timeout: 30)); XCTAssertTrue(ok.isHittable)
        phoneCapture(in: app, id: phoneCaptureID("pages", "cannot-open-file"), theme: theme, fixture: [],
            extra: ["provider_fixture": "explicit-malformed-package"])
        workspaceActivate(ok)
        XCTAssertTrue(ok.waitForNonExistence(timeout: 10))
        phoneOpenProviderFile("Unsupported.capy", token: token, provider: provider, in: app)
        let copy = app.buttons["copy-original-package"]
        XCTAssertTrue(copy.waitForExistence(timeout: 30)); XCTAssertTrue(copy.isEnabled)
        XCTAssertTrue(app.descendants(matching: .any)["preserved-package-preview"].firstMatch.exists)
        phoneCapture(in: app, id: phoneCaptureID("pages", "preserved-package-preview"), theme: theme, fixture: [],
            extra: ["provider_fixture": "Unsupported.capy"])
    }
}

extension XCTestCase {
    @MainActor func findPhoneMenuRow(_ label: String, in app: XCUIApplication) -> XCUIElement? {
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        guard menu.exists, let snapshot = try? menu.snapshot() else { return nil }
        func containsText(_ node: any XCUIElementSnapshot) -> Bool {
            node.elementType == .staticText && node.label == label || node.children.contains(where: containsText)
        }
        func identifier(_ node: any XCUIElementSnapshot) -> String? {
            if node.elementType == .button, node.identifier.hasPrefix("phone-menu-row-"),
                node.label == label || containsText(node) { return node.identifier }
            return node.children.lazy.compactMap(identifier).first
        }
        return identifier(snapshot).map { app.buttons[$0].firstMatch }
    }

    @MainActor func phoneMenuRow(_ label: String, in app: XCUIApplication) -> XCUIElement {
        let ready = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in self.findPhoneMenuRow(label, in: app) != nil }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 5), .completed, label)
        return findPhoneMenuRow(label, in: app) ?? app.buttons.matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND label == %@", "phone-menu-row-", label)).firstMatch
    }

    @MainActor func phoneMenuRoot(in app: XCUIApplication) -> XCUIElement {
        workspaceActivate(app.buttons["phone-capy"])
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        let scroll = app.descendants(matching: .any)["phone-menu-scroll"].firstMatch
        XCTAssertTrue(scroll.waitForExistence(timeout: 10))
        let back = app.buttons["phone-menu-back"]
        while back.exists {
            guard let current = phoneMenuID(title: menu.label), let parent = phoneMenuParentID(current),
                let title = parent.menu?["title"] as? String else { return scroll }
            workspaceActivate(back)
            phoneWaitForMenuTitle(title, in: app)
        }
        phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
        return scroll
    }

    @MainActor func phoneChooseFingerMode(_ mode: String, in app: XCUIApplication) {
        if !app.descendants(matching: .any)["phone-menu"].firstMatch.exists { _ = phoneMenuRoot(in: app) }
        workspaceActivate(app.buttons["phone-finger"])
        phoneWaitForMenuTitle("One-finger drag", in: app)
        XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-row-")).count, 5)
        workspaceActivate(phoneMenuRow(mode, in: app))
        phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
        XCTAssertFalse(app.buttons["phone-menu-back"].exists)
        workspaceActivate(app.buttons["phone-finger"])
        phoneWaitForMenuTitle("One-finger drag", in: app)
        XCTAssertTrue(phoneMenuRow(mode, in: app).isSelected)
        XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND selected == YES", "phone-menu-row-")).count, 1)
        workspaceActivate(app.buttons["phone-menu-back"])
        phoneWaitForMenuTitle(app.buttons["phone-capy"].label, in: app)
    }

    @MainActor func phoneWaitForMenuTitle(_ title: String, in app: XCUIApplication) {
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        let settled = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in menu.exists && menu.label == title }, object: app)
        XCTAssertEqual(XCTWaiter.wait(for: [settled], timeout: 10), .completed, "The shared menu must publish page \(title)")
    }

    @MainActor func phoneMenuCommand(label: String, in app: XCUIApplication) {
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        let scroll = menu.exists ? app.descendants(matching: .any)["phone-menu-scroll"].firstMatch : phoneMenuRoot(in: app)
        func find() -> Bool {
            if let row = findPhoneMenuRow(label, in: app) {
                revealEditorControl(row, in: scroll); XCTAssertTrue(row.isEnabled); workspaceActivate(row); return true
            }
            let parent = menu.label
            let children = menu.descendants(matching: .any).matching(identifier: "phone-menu-submenu").allElementsBoundByIndex
                .map { $0.buttons.firstMatch }.filter { $0.exists && $0.isEnabled }.map(\.identifier)
            for identifier in children {
                let child = app.buttons[identifier].firstMatch
                guard let title = phoneMenuTitle(for: child) else { return false }
                revealEditorControl(child, in: scroll); workspaceActivate(child)
                phoneWaitForMenuTitle(title, in: app)
                if find() { return true }
                workspaceActivate(app.buttons["phone-menu-back"])
                phoneWaitForMenuTitle(parent, in: app)
            }
            return false
        }
        XCTAssertTrue(find(), "The shared phone menu must expose \(label)")
    }

    @MainActor func phoneAlertCommand(_ application: String, label: String, in app: XCUIApplication) {
        let scroll = phoneMenuRoot(in: app)
        let section = phoneMenuRow(application, in: app)
        guard let title = phoneMenuTitle(for: section) else { return }
        revealEditorControl(section, in: scroll); workspaceActivate(section)
        phoneWaitForMenuTitle(title, in: app)
        phoneMenuCommand(label: label, in: app)
    }

    @MainActor func phoneOpenProviderFile(_ name: String, token: String, provider: String, in app: XCUIApplication) {
        phoneAlertCommand("File", label: "Open…", in: app)
        phoneChooseProviderFile(name, token: token, provider: provider, in: app)
    }

    @MainActor func phoneChooseProviderFile(_ name: String, token: String, provider: String, submitSelection: Bool = true, in app: XCUIApplication) {
        let root = fileProviderRoot(provider, in: app)
        if !root.exists {
            let local = app.cells.matching(NSPredicate(format: "identifier IN %@",
                ["DOC.sidebar.item.On My iPhone", "DOC.sidebar.item.On My iPad"])).firstMatch
            if !local.waitForExistence(timeout: 3) {
                let browse = app.buttons["Browse"].firstMatch
                XCTAssertTrue(browse.waitForExistence(timeout: 10)); workspaceActivate(browse)
            }
            let location = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                root.exists || local.exists
            }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [location], timeout: 15), .completed)
            if !root.exists { workspaceActivate(local) }
        }
        XCTAssertTrue(root.waitForExistence(timeout: 15)); workspaceActivate(root)
        let directory = app.cells["Capy Files " + token + ", Folder"]
        XCTAssertTrue(directory.waitForExistence(timeout: 10)); workspaceActivate(directory)
        workspaceActivate(revealFileProviderFile(name, in: app))
        if submitSelection { submitFileProviderSelection(in: app) }
    }
}
