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
        for command in ["repair_source_profile","rasterize_source"] {
            actions.append(["type":"customize","action":["type":"picker_select","control":["kind":"command","command":command],"selected":true]])
        }
        actions.append(["type":"customize","action":["type":"confirm_tools"]])
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"]=String(data:try JSONSerialization.data(withJSONObject:actions),encoding:.utf8)!
        app.launch();capturePaintEditor(in:app)
        let title=app.staticTexts["document-title"]
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
        workspaceActivate(app.buttons["layer-Import image as layer"]);chooseFile(photo)
        expectation(for:NSPredicate(format:"count == 3"),evaluatedWith:rows);waitForExpectations(timeout:30)
        let original=editorPixels(in:app)
        func command(_ label:String) {
            let button=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-",label)).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout:10));XCTAssertTrue(button.isEnabled);workspaceActivate(button)
        }
        let preview=app.buttons["document-color-preview"],apply=app.buttons["document-color-apply"]
        func compare() {
            workspaceActivate(preview)
            expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:apply);waitForExpectations(timeout:90)
            XCTAssertTrue(app.staticTexts["Before"].exists && app.staticTexts["After"].exists)
        }
        command("Repair Source Profile…")
        workspaceActivate(app.buttons["source-profile-import"])
        XCTAssertTrue(open.waitForExistence(timeout:15));app.typeKey(XCUIKeyboardKey.escape.rawValue,modifierFlags:[])
        XCTAssertTrue(open.waitForNonExistence(timeout:10));XCTAssertTrue(preview.exists)
        workspaceActivate(app.buttons["source-profile-import"]);chooseFile(profile)
        let picker=app.popUpButtons["photo-profile-space"]
        expectation(for:NSPredicate(format:"value CONTAINS %@","P3"),evaluatedWith:picker);waitForExpectations(timeout:20)
        compare();attachEditor(in:app,name:"source-profile-icc-comparison")
        let alternate=root.appendingPathComponent("Alternate.icc")
        try (CGColorSpace(name:CGColorSpace.sRGB)!.copyICCData()! as Data).write(to:alternate)
        // Replacing the imported profile leaves the same picker selection key;
        // its import notification must still invalidate the prepared result.
        workspaceActivate(app.buttons["source-profile-import"]);chooseFile(alternate)
        expectation(for:NSPredicate(format:"value CONTAINS %@","sRGB"),evaluatedWith:picker);waitForExpectations(timeout:20)
        XCTAssertFalse(apply.isEnabled);compare()
        workspaceActivate(app.buttons["Cancel"].firstMatch);XCTAssertTrue(preview.waitForNonExistence(timeout:10))
        XCTAssertEqual(editorPixels(in:app),original)
        command("Repair Source Profile…");workspaceActivate(app.popUpButtons["photo-profile-space"])
        workspaceActivate(app.menuItems["Display P3"].firstMatch);compare();workspaceActivate(apply)
        XCTAssertTrue(preview.waitForNonExistence(timeout:30))
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app) != original },evaluatedWith:app);waitForExpectations(timeout:20)
        let repaired=editorPixels(in:app)
        editorHistory("Undo",in:app)
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app)==original },evaluatedWith:app);waitForExpectations(timeout:20)
        editorHistory("Redo",in:app)
        expectation(for:NSPredicate { _,_ in self.editorPixels(in:app)==repaired },evaluatedWith:app);waitForExpectations(timeout:20)
        editorMenu(in:app,menu:"Select",id:"select_all",label:"Select all pixels")
        editorMenu(in:app,menu:"Edit",id:"fill_selection",label:"Fill selection")
        editorMenu(in:app,menu:"Select",id:"deselect",label:"Deselect pixels")
        command("Repair Source Profile…");workspaceActivate(app.popUpButtons["photo-profile-space"])
        workspaceActivate(app.menuItems["ProPhoto RGB"].firstMatch);compare()
        XCTAssertEqual(apply.label,"Add Corrected Source");attachEditor(in:app,name:"source-repair-preserves-painted-layer")
        workspaceActivate(apply);XCTAssertTrue(preview.waitForNonExistence(timeout:30))
        expectation(for:NSPredicate(format:"count == 4"),evaluatedWith:rows);waitForExpectations(timeout:15)
        command("Rasterize Source…");compare();attachEditor(in:app,name:"source-rasterize-comparison")
        XCTAssertEqual(apply.label,"Rasterize");workspaceActivate(apply);XCTAssertTrue(preview.waitForNonExistence(timeout:30))
        let rasterize=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-","Rasterize Source…")).firstMatch
        XCTAssertFalse(rasterize.isEnabled);editorHistory("Undo",in:app)
        expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:rasterize);waitForExpectations(timeout:20)
        editorHistory("Redo",in:app);XCTAssertFalse(rasterize.isEnabled)
        XCTAssertEqual(try Data(contentsOf:photo),originalBytes);XCTAssertEqual(try Data(contentsOf:profile),profileBytes)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #endif
    }
}
