import XCTest
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

extension XCTestCase {
    @MainActor func checkNativeSourceEditing(in app: XCUIApplication) throws {
        #if os(macOS)
        let root=FileManager.default.temporaryDirectory.appendingPathComponent("Capy source "+UUID().uuidString)
        try FileManager.default.createDirectory(at:root,withIntermediateDirectories:false)
        defer {try? FileManager.default.removeItem(at:root)}
        let photo=root.appendingPathComponent("Retained original.png"),profile=root.appendingPathComponent("Display P3.icc")
        let profileBytes=CGColorSpace(name:CGColorSpace.displayP3)!.copyICCData()! as Data
        try profileBytes.write(to:profile)
        var actions:[[String:Any]]=[
            ["type":"set_theme","theme":"light"],
            ["type":"customize","action":["type":"insert_tools","panel":"commands","before":NSNull()]]]
        for command in ["open_document","repair_source_profile","rasterize_source","select_all","fill_selection","deselect"] {
            actions.append(["type":"customize","action":["type":"picker_select","control":["kind":"command","command":command],"selected":true]])
        }
        actions.append(["type":"customize","action":["type":"confirm_tools"]])
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"]=String(data:try JSONSerialization.data(withJSONObject:actions),encoding:.utf8)!
        app.launch();capturePaintEditor(in:app)
        let title=editorDocumentTitle(in: app)
        let size=(title.value as? String ?? title.label).components(separatedBy:" · ").last!.components(separatedBy:" × ").compactMap(Int.init)
        XCTAssertEqual(size.count,2)
        let context=try XCTUnwrap(CGContext(data:nil,width:size[0],height:size[1],bitsPerComponent:8,bytesPerRow:size[0]*4,
            space:CGColorSpace(name:CGColorSpace.sRGB)!,bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(red:0.5,green:0.7,blue:0.3,alpha:1);context.fill(CGRect(x:0,y:0,width:size[0],height:size[1]))
        let output=try XCTUnwrap(CGImageDestinationCreateWithURL(photo as CFURL,UTType.png.identifier as CFString,1,nil))
        CGImageDestinationAddImage(output,try XCTUnwrap(context.makeImage()),nil);XCTAssertTrue(CGImageDestinationFinalize(output))
        let originalBytes=try Data(contentsOf:photo)
        let rows=app.descendants(matching:.any).matching(NSPredicate(format:"identifier BEGINSWITH %@","layer-row-"))
        let open=app.windows.buttons["OKButton"].firstMatch
        func chooseFile(_ url:URL) {
            XCTAssertTrue(open.waitForExistence(timeout:15))
            app.typeKey("g",modifierFlags:[.command,.shift]);app.typeText(url.path+"\n");workspaceActivate(open)
            XCTAssertTrue(open.waitForNonExistence(timeout:15))
        }
        func command(_ label:String) {
            let button=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-",label)).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout:10));XCTAssertTrue(button.isEnabled);workspaceActivate(button)
        }
        command("Open…");chooseFile(photo)
        let drawingName=photo.deletingPathExtension().lastPathComponent
        expectation(for:NSPredicate(format:"label CONTAINS %@ OR value CONTAINS %@",drawingName,drawingName),evaluatedWith:title)
        waitForExpectations(timeout:30)
        let initialCount=rows.count,original=editorPixels(in:app)
        let apply=app.buttons["document-color-apply"]
        func compare() {
            expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:apply);waitForExpectations(timeout:90)
            XCTAssertTrue(app.staticTexts["Before"].exists && app.staticTexts["After"].exists)
        }
        command("Repair Source Profile…")
        workspaceActivate(app.buttons["source-profile-import"])
        XCTAssertTrue(open.waitForExistence(timeout:15));app.typeKey(XCUIKeyboardKey.escape.rawValue,modifierFlags:[])
        XCTAssertTrue(open.waitForNonExistence(timeout:10));XCTAssertTrue(apply.exists)
        workspaceActivate(app.buttons["source-profile-import"]);chooseFile(profile)
        let picker=app.menuButtons["photo-profile-space"]
        expectation(for:NSPredicate(format:"label CONTAINS %@ OR title CONTAINS %@","P3","P3"),evaluatedWith:picker);waitForExpectations(timeout:20)
        compare();attachEditor(in:app,name:"source-profile-icc-comparison")
        let alternate=root.appendingPathComponent("Alternate.icc")
        try (CGColorSpace(name:CGColorSpace.sRGB)!.copyICCData()! as Data).write(to:alternate)
        workspaceActivate(app.buttons["source-profile-import"]);chooseFile(alternate)
        expectation(for:NSPredicate(format:"label CONTAINS %@ OR title CONTAINS %@","sRGB","sRGB"),evaluatedWith:picker);waitForExpectations(timeout:20)
        XCTAssertFalse(apply.isEnabled);compare()
        workspaceActivate(app.windows.buttons["Cancel"].firstMatch);XCTAssertTrue(apply.waitForNonExistence(timeout:10))
        XCTAssertEqual(editorPixels(in:app),original)
        command("Repair Source Profile…");compare();workspaceActivate(picker)
        let displayP3=app.menuItems.matching(identifier:"Display P3");XCTAssertTrue(displayP3.firstMatch.waitForExistence(timeout:10))
        workspaceActivate(displayP3.element(boundBy:displayP3.count-1));compare();workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout:30))
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app) != original },evaluatedWith:app);waitForExpectations(timeout:20)
        let repaired=editorPixels(in:app)
        editorHistory("Undo",in:app)
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app)==original },evaluatedWith:app);waitForExpectations(timeout:20)
        editorHistory("Redo",in:app)
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app)==repaired },evaluatedWith:app);waitForExpectations(timeout:20)
        command("Select all pixels");command("Fill selection");command("Deselect pixels")
        command("Repair Source Profile…");compare();workspaceActivate(picker)
        workspaceActivate(app.menuItems["ProPhoto RGB"].firstMatch);compare()
        XCTAssertEqual(apply.label,"Add Corrected Source");attachEditor(in:app,name:"source-repair-preserves-painted-layer")
        workspaceActivate(apply);XCTAssertTrue(apply.waitForNonExistence(timeout:30))
        expectation(for:NSPredicate(format:"count == %d",initialCount+1),evaluatedWith:rows);waitForExpectations(timeout:15)
        command("Rasterize Source…");compare();attachEditor(in:app,name:"source-rasterize-comparison")
        XCTAssertEqual(apply.label,"Rasterize");workspaceActivate(apply);XCTAssertTrue(apply.waitForNonExistence(timeout:30))
        let rasterize=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-","Rasterize Source…")).firstMatch
        XCTAssertFalse(rasterize.isEnabled);editorHistory("Undo",in:app)
        expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:rasterize);waitForExpectations(timeout:20)
        editorHistory("Redo",in:app)
        expectation(for:NSPredicate(format:"enabled == NO"),evaluatedWith:rasterize);waitForExpectations(timeout:20)
        XCTAssertEqual(try Data(contentsOf:photo),originalBytes);XCTAssertEqual(try Data(contentsOf:profile),profileBytes)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #endif
    }
}


extension XCTestCase {
    @MainActor func checkNativeFloatSourceCaption(in app: XCUIApplication, theme: String) async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let token = environment["CAPY_FILE_TEST_TOKEN"], UUID(uuidString: token) != nil,
            let expected = environment["CAPY_UI_FLOAT_SOURCE_CAPTION"], !expected.isEmpty else {
            throw XCTSkip("The private unprofiled F32 EXR and shared caption are not supplied")
        }
        let folder = "Capy Files " + token
        #if os(macOS)
        guard let directory = environment["CAPY_UI_PACKAGE_DIRECTORY"] else { throw XCTSkip("No private EXR directory") }
        let root = URL(fileURLWithPath: directory, isDirectory: true)
        XCTAssertTrue(directory.hasPrefix("/") && root.lastPathComponent == folder)
        #else
        guard let provider = environment["CAPY_UI_PACKAGE_PROVIDER"] else { throw XCTSkip("No private EXR provider") }
        XCTAssertTrue(provider.hasPrefix("Capy Test Files "))
        let root = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent(folder, isDirectory: true)
        #endif
        let file = root.appendingPathComponent("Unprofiled-f32.exr")
        func retainedBytes() async throws -> Data {
            try await Task.detached(priority: .utility) {
                let values = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true,
                    let count = values.fileSize, (1...65536).contains(count) else { throw CocoaError(.fileReadCorruptFile) }
                return try Data(contentsOf: file)
            }.value
        }
        let exists = await Task.detached(priority: .utility) { FileManager.default.fileExists(atPath: file.path) }.value
        guard exists else { throw XCTSkip("The private unprofiled F32 EXR is absent") }
        let original = try await retainedBytes()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"preferences","action":{"type":"edit","id":"language","value":1}},{"type":"preferences","action":{"type":"edit","id":"missing_profile","value":1}},{"type":"set_color","rgba":[0.1,0.3,0.9,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        let title = editorDocumentTitle(in: app), initialTitle = title.value as? String ?? title.label
        let untouched = editorPixels(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) != untouched }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let painted = editorPixels(in: app)
        #if !os(macOS)
        var providerCaptured = false
        #endif
        func openFixture() {
            editorMenu(in: app, menu: "File", id: "open_document", label: "Open…")
            #if os(macOS)
            let open = app.windows.buttons["OKButton"].firstMatch
            XCTAssertTrue(open.waitForExistence(timeout: 15)); goToFolder(file, in: app)
            workspaceActivate(open); XCTAssertTrue(open.waitForNonExistence(timeout: 20))
            #else
            func folderTitle(_ value: String) -> XCUIElement {
                app.otherElements["DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: " + value]
            }
            let local = app.cells["DOC.sidebar.item.On My iPad"]
            XCTAssertTrue(local.waitForExistence(timeout: 15)); workspaceActivate(local)
            XCTAssertTrue(folderTitle("On My iPad").waitForExistence(timeout: 10))
            if !providerCaptured {
                providerCaptured = true
                let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
                capture.name = "unprofiled-f32-provider-first-open-" + theme
                capture.lifetime = .keepAlways; add(capture)
                print(app.debugDescription)
            }
            workspaceActivate(fileProviderRoot(provider, in: app))
            XCTAssertTrue(folderTitle(provider).waitForExistence(timeout: 10))
            workspaceActivate(app.cells[folder + ", Folder"])
            XCTAssertTrue(folderTitle(folder).waitForExistence(timeout: 10))
            workspaceActivate(revealFileProviderFile("Unprofiled-f32.exr", in: app))
            submitFileProviderSelection(in: app)
            #endif
            XCTAssertTrue(app.buttons["photo-profile-use"].waitForExistence(timeout: 30))
            XCTAssertTrue(app.staticTexts[expected].waitForExistence(timeout: 15))
            XCTAssertTrue(app.descendants(matching: .any)["photo-profile-space"].firstMatch.exists)
        }
        openFixture()
        attachEditor(in: app, name: "unprofiled-f32-caption-" + theme)
        #if os(macOS)
        workspaceActivate(app.windows.buttons["Cancel"].firstMatch)
        #else
        workspaceActivate(app.buttons["Cancel"].firstMatch)
        #endif
        XCTAssertTrue(app.buttons["photo-profile-use"].waitForNonExistence(timeout: 15))
        XCTAssertEqual(editorDocumentTitle(in: app).value as? String ?? editorDocumentTitle(in: app).label, initialTitle)
        XCTAssertEqual(editorPixels(in: app), painted)
        editorHistory("Undo", in: app); expectPixels(painted, in: app)
        editorHistory("Undo", in: app); expectPixels(untouched, in: app)
        editorHistory("Redo", in: app); expectPixels(painted, in: app)
        editorHistory("Redo", in: app); expectPixels(painted, in: app)
        openFixture(); workspaceActivate(app.buttons["photo-profile-use"])
        XCTAssertTrue(app.buttons["photo-profile-use"].waitForNonExistence(timeout: 30))
        let importedTitle = editorDocumentTitle(in: app)
        expectation(for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "Unprofiled-f32", "Unprofiled-f32"), evaluatedWith: importedTitle)
        waitForExpectations(timeout: 30)
        attachEditor(in: app, name: "unprofiled-f32-used-" + theme)
        let retained = try await retainedBytes()
        XCTAssertEqual(retained, original)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
