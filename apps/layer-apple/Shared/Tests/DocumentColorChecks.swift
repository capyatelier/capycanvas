import XCTest

extension XCTestCase {
    @MainActor func checkNativeDocumentColor(in app: XCUIApplication) throws {
        var actions: [[String: Any]] = [
            ["type":"set_theme","theme":"light"],
            ["type":"customize","action":["type":"insert_tools","panel":"commands","before":NSNull()]]
        ]
        for command in ["assign_profile","convert_color_space","change_bit_depth","document_properties"] {
            actions.append(["type":"customize","action":["type":"picker_select","control":["kind":"command","command":command],"selected":true]])
        }
        actions += [["type":"customize","action":["type":"confirm_tools"]],
            ["type":"color","action":["op":"definition","color":["space":"Srgb","rgba":[0.7,0.3,0.15,1]]]]]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data:try JSONSerialization.data(withJSONObject:actions),encoding:.utf8)!
        app.launch(); capturePaintEditor(in:app)
        // Startup actions configure controls before all GPU work is ready.
        // Seed artwork through the same ready native menus as document tests.
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app)
            return Int(sample[0]) > Int(sample[2]) + 80
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        func command(_ label:String) {
            let button=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-",label)).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout:15))
            expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:button);waitForExpectations(timeout:30)
            workspaceActivate(button)
        }
        func choose(_ id:String,_ label:String) {
            let picker=app.descendants(matching:.any).matching(identifier:id).firstMatch
            XCTAssertTrue(picker.waitForExistence(timeout:10));workspaceActivate(picker)
            #if os(macOS)
            workspaceActivate(app.menuItems[label].firstMatch)
            #else
            workspaceActivate(app.buttons[label].firstMatch)
            #endif
        }
        let preview=app.buttons["document-color-preview"],apply=app.buttons["document-color-apply"]
        func compare() {
            workspaceActivate(preview)
            expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:apply);waitForExpectations(timeout:90)
            XCTAssertTrue(app.staticTexts["Before"].exists && app.staticTexts["After"].exists)
        }
        let before=editorPixels(in:app)
        command("Assign Profile…");choose("document-color-space","ProPhoto RGB");compare()
        attachEditor(in:app,name:"document-assign-comparison")
        workspaceActivate(app.buttons["Cancel"].firstMatch)
        XCTAssertTrue(preview.waitForNonExistence(timeout:10));XCTAssertEqual(editorPixels(in:app),before)
        command("Assign Profile…");choose("document-color-space","ProPhoto RGB");compare();workspaceActivate(apply)
        XCTAssertTrue(preview.waitForNonExistence(timeout:30))
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app) != before },evaluatedWith:app);waitForExpectations(timeout:20)
        let assigned=editorPixels(in:app)
        editorHistory("Undo",in:app)
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app)==before },evaluatedWith:app);waitForExpectations(timeout:90)
        editorHistory("Redo",in:app)
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app)==assigned },evaluatedWith:app);waitForExpectations(timeout:90)
        command("Change Bit Depth…");choose("document-color-depth","16-bit SDR");compare();workspaceActivate(apply)
        XCTAssertTrue(preview.waitForNonExistence(timeout:30))
        command("Document Properties…")
        XCTAssertTrue(app.staticTexts["16-bit integer SDR"].waitForExistence(timeout:15))
        XCTAssertTrue(app.staticTexts["ProPhoto RGB"].exists)
        attachEditor(in:app,name:"document-properties-after-depth")
        workspaceActivate(app.buttons["Done"].firstMatch)
        command("Convert Color Space…");choose("document-color-space","Display P3");compare()
        attachEditor(in:app,name:"document-convert-comparison")
        workspaceActivate(apply);XCTAssertTrue(preview.waitForNonExistence(timeout:30))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
