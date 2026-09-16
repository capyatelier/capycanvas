import Foundation
import QuartzCore

/// Real owner/coordinator, complete previews and color history on Mac Metal for
/// both Apple policies. Files and persistence belong only to this fixture.
@main struct DocumentColorOwnerChecks {
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            store.edit(action) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
            }
        }
    }
    @MainActor static func wait(_ label: String, store: EditorStore, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(45)
        while !ready() {
            try require(Date() < deadline && store.failure == nil, store.failure ?? "Timed out: \(label)")
            let prepared = await withCheckedContinuation { done in store.native!.flushPersistence { done.resume(returning: $0) } }
            try require(prepared, "Prepare canvas: \(label)")
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0,1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-document-color-\(UUID())")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root), managedWorkspaces: false)
            let native = store.native!
            let surface = CAMetalLayer(); surface.bounds = CGRect(x:0,y:0,width:128,height:128)
            native.attach(surface,width:128,height:128,scale:1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            let deadline = Date().addingTimeInterval(45)
            while !store.snapshot["shaders_ready"].bool {
                try require(Date() < deadline && store.failure == nil, store.failure ?? "Metal startup")
                let now=FrameTrace.now()
                await withCheckedContinuation { done in native.frame(now:now,target:now+16_666_667) { _,_,_ in done.resume() } }
                try await Task.sleep(for:.milliseconds(10))
            }
            let master=root.appendingPathComponent("Editable.capy")
            store.projectFiles = ProjectFiles(store:store,dialogs:.init(open:{$0(nil)},save:{_,_,done in done(master)},create:{_,done in
                done(JSON(["extent":[128,96],"color":["space":"DisplayP3","depth":"U16"],"background":"White"]))
            }))
            func invoke(_ command:String) async throws { try await edit(store,["type":"invoke","command":command]) }
            func idle() async throws {
                try await wait("Document completion",store:store) { !store.projectFiles.busy && !store.state["requests"].array.contains {$0["kind"]["type"].string=="document"} }
                try require(store.projectFiles.error == nil,store.projectFiles.error ?? "")
            }
            func dialog(_ command:String) async throws -> DocumentColorController {
                try await invoke(command)
                try await wait("Color form",store:store) { store.projectFiles.colorEditor?.loaded == true || store.projectFiles.colorEditor?.error != nil }
                let editor=store.projectFiles.colorEditor!
                try require(editor.error == nil,editor.error ?? "")
                return editor
            }
            func prepared(_ editor:DocumentColorController,_ choice:JSON,copy:Bool=false) async throws {
                editor.prepare(choice,copy:copy)
                try await wait("Complete comparison",store:store) { editor.ready || editor.error != nil }
                try require(editor.ready && editor.previews.count==2,editor.error ?? "Two complete previews required")
            }
            try await invoke("new_document");try await idle()
            try await edit(store,["type":"color","action":["op":"definition","color":["space":"DisplayP3","rgba":[0.7,0.3,0.15,0.8]]]])
            try await invoke("select_all")
            try await edit(store,["type":"layer","action":["op":"fill_selection"]])
            try await invoke("deselect")
            let captured = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning:$0) } }
            try require(captured,"Finish painted raster before saving")
            try await invoke("save_document");try await idle()
            store.layerThumbnails.show(token:"color-history",id:1)
            try await wait("Initial layer thumbnail",store:store) {store.layerThumbnails.images["1:false"] != nil}
            let original=try Data(contentsOf:master)
            let epoch=store.state["document_file"]["epoch"].uint
            let assignment=JSON(["Assign":"ProPhoto"])
            var editor=try await dialog("assign_profile")
            try require(editor.color["space"].string=="DisplayP3" && editor.color["depth"].string=="U16","Load current document color")
            try await prepared(editor,assignment)
            try require(store.state["colors"]["rgb_space"].string=="DisplayP3","Preview must not alter live color")
            editor.cancel();try await idle()
            try require(store.state["document_file"]["epoch"].uint==epoch && !store.state["document_file"]["modified"].bool,"Cancel must preserve the saved document")
            editor=try await dialog("assign_profile");try await prepared(editor,assignment);editor.apply();try await idle()
            try require(store.state["colors"]["rgb_space"].string=="ProPhoto" && store.state["document_file"]["modified"].bool,"Assignment must apply once")
            try await invoke("undo");try await idle();try require(store.state["colors"]["rgb_space"].string=="DisplayP3","Color Undo must use prepared renderer")
            try await invoke("redo");try await idle();try require(store.state["colors"]["rgb_space"].string=="ProPhoto","Color Redo must use prepared renderer")
            let conversion=JSON(["Convert":["space":"Srgb","options":["intent":"RelativeColorimetric","black_point_compensation":false]]])
            editor=try await dialog("convert_color_space");try await prepared(editor,conversion,copy:true)
            editor.saveCopy(to:master)
            try require(editor.error != nil && !editor.busy && editor.ready,"Reject overwrite of editable master before writing")
            let occupied=root.appendingPathComponent("Existing.capy")
            let sentinel=Data("Keep existing destination".utf8);try sentinel.write(to:occupied)
            editor.saveCopy(to:occupied,access:root)
            try await wait("Existing folder destination",store:store) {!editor.busy}
            try require(editor.error != nil && (try Data(contentsOf:occupied))==sentinel,"Folder selection must not overwrite an existing file")
            try await prepared(editor,conversion,copy:true)
            let copy=root.appendingPathComponent("Converted.capy")
            editor.saveCopy(to:copy,access:root);try await idle()
            try require(try Data(contentsOf:master)==original,"Flattened copy must not modify master bytes")
            try require(FileManager.default.fileExists(atPath:copy.path) && store.state["colors"]["rgb_space"].string=="ProPhoto","Save Copy leaves editable document active")
            editor=try await dialog("change_bit_depth")
            // A mismatched operation must remain retryable through a fresh job.
            editor.prepare(assignment)
            try await wait("Invalid color operation",store:store) {editor.error != nil}
            try await prepared(editor,JSON(["Depth":["depth":"U8","dither":"Stochastic8"]]));editor.apply();try await idle()
            editor=try await dialog("document_properties")
            try require(editor.rows.contains {$0[0].string=="Bit depth" && $0[1].string=="8-bit integer SDR"},"Properties must report updated precision")
            editor.cancel();try await idle()
            try await invoke("undo");try await idle()
            editor=try await dialog("document_properties")
            try require(editor.rows.contains {$0[0].string=="Bit depth" && $0[1].string=="16-bit integer SDR"},"Undo must restore higher precision")
            editor.cancel();try await idle()
            try await wait("Thumbnail after color history",store:store) {store.layerThumbnails.images["1:false"] != nil}
            editor=try await dialog("convert_color_space")
            editor.prepare(conversion);editor.cancel();try await idle()
            try require(store.state["colors"]["rgb_space"].string=="ProPhoto" && store.state["document_file"]["epoch"].uint==epoch,"Cancel while preparing must leave the same document intact")
            print("PASS platform \(platform): color previews, Cancel/Apply, exact history route, copy/save protection, retry, properties and worker cancellation")
        }
    }
}
