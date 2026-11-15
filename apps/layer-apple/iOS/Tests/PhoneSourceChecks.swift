import XCTest

extension EditorLaunchTests {
    @MainActor func testPhoneSourceDialogsLight() async throws { try await checkPhoneSourceDialogs(theme: "light") }
    @MainActor func testPhoneSourceDialogsDark() async throws { try await checkPhoneSourceDialogs(theme: "dark") }

    @MainActor private func checkPhoneSourceDialogs(theme: String) async throws {
        let environment = ProcessInfo.processInfo.environment
        let token = try XCTUnwrap(environment["CAPY_FILE_TEST_TOKEN"])
        XCTAssertNotNil(UUID(uuidString: token))
        let provider = try XCTUnwrap(environment["CAPY_UI_PACKAGE_PROVIDER"])
        XCTAssertEqual(provider, "Capy Test Files " + token)
        let folder = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Capy Files " + token, isDirectory: true)
        let file = folder.appendingPathComponent("Unprofiled-f32.exr")
        let original = try await Task.detached {
            let facts = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
            guard facts.isRegularFile == true, facts.isSymbolicLink != true,
                let size = facts.fileSize, (1...65536).contains(size) else { throw CocoaError(.fileReadCorruptFile) }
            return try Data(contentsOf: file)
        }.value
        let actions: [[String: Any]] = [["type": "preferences", "action": ["type": "edit", "id": "missing_profile", "value": 1]]]
        let app = try phoneCaptureApplication(theme: theme, actions: actions)
        defer { app.terminate() }
        func any(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func activate(_ id: String) {
            let control = app.buttons[id].firstMatch
            XCTAssertTrue(control.waitForExistence(timeout: 15), id)
            if !control.isHittable, let scroll = app.scrollViews.containing(.any, identifier: id).allElementsBoundByIndex.last {
                revealEditorControl(control, in: scroll)
            }
            workspaceActivate(control)
        }
        func capture(_ state: String, providerFixture: String = "Unprofiled-f32.exr") {
            phoneCapture(in: app, id: phoneCaptureID("pages", state), theme: theme, fixture: actions,
                extra: ["provider_fixture": providerFixture])
        }
        phoneDrawingFixtures.removeValue(forKey: try XCTUnwrap(app.launchEnvironment["CAPY_STORAGE_DIR"]))
        phoneOpenProviderFile("Unprofiled-f32.exr", token: token, provider: provider, in: app)
        XCTAssertTrue(app.buttons["photo-profile-use"].waitForExistence(timeout: 30))
        XCTAssertTrue(any("photo-profile-space").exists)
        if let caption = environment["CAPY_UI_FLOAT_SOURCE_CAPTION"] {
            XCTAssertTrue(app.staticTexts[caption].waitForExistence(timeout: 10))
        }
        capture("untagged-source-profile")
        activate("color-profile-library")
        XCTAssertTrue(app.buttons["profile-library-done"].waitForExistence(timeout: 15))
        activate("profile-library-done")
        activate("photo-profile-use")
        XCTAssertTrue(app.buttons["photo-profile-use"].waitForNonExistence(timeout: 30))
        let canvas = any("canvas")
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 60)
        let retained = try await Task.detached { try Data(contentsOf: file) }.value
        XCTAssertEqual(retained, original)
        let photo = folder.appendingPathComponent("prophoto16.png")
        let photoOriginal = try await Task.detached { try Data(contentsOf: photo) }.value
        phoneOpenProviderFile(photo.lastPathComponent, token: token, provider: provider, in: app)
        expectation(for: NSPredicate { _, _ in
            guard canvas.value as? String == "Canvas ready" else { return false }
            let samples = self.editorPixelSamples(in: app, at: [CGPoint(x: 0.3, y: 0.5), CGPoint(x: 0.7, y: 0.5)], size: 8)
            return samples[0] != samples[1]
        }, evaluatedWith: app)
        waitForExpectations(timeout: 60)
        for (label, state) in [("Repair Source Profile…", "repair-source-profile"), ("Rasterize Source…", "rasterize-source")] {
            phoneMenuCommand(label: label, in: app)
            let apply = app.buttons["document-color-apply"]
            XCTAssertTrue(apply.waitForExistence(timeout: 30))
            if state == "repair-source-profile" {
                let ready = expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: apply)
                await fulfillment(of: [ready], timeout: 90)
                activate("photo-profile-space")
                workspaceActivate(app.buttons["sRGB"].firstMatch)
            }
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: apply)
            waitForExpectations(timeout: 90)
            capture(state, providerFixture: photo.lastPathComponent)
            activate("document-color-cancel")
            XCTAssertTrue(apply.waitForNonExistence(timeout: 15))
        }
        for (label, state, picker, choice) in [
            ("Assign Profile…", "assign-profile", "document-color-space", "sRGB"),
            ("Convert Color Space…", "convert-color-space", "document-color-space", "sRGB"),
            ("Change Bit Depth…", "change-bit-depth", "document-color-depth", "8-bit SDR")
        ] {
            phoneAlertCommand("Image", label: label, in: app)
            let apply = app.buttons["document-color-apply"]
            XCTAssertTrue(apply.waitForExistence(timeout: 30))
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: any(picker))
            waitForExpectations(timeout: 30)
            activate(picker)
            workspaceActivate(app.buttons[choice].firstMatch)
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: apply)
            waitForExpectations(timeout: 90)
            capture(state, providerFixture: photo.lastPathComponent)
            activate("document-color-cancel")
            XCTAssertTrue(apply.waitForNonExistence(timeout: 15))
        }
        let photoRetained = try await Task.detached { try Data(contentsOf: photo) }.value
        XCTAssertEqual(photoRetained, photoOriginal)
    }
}
