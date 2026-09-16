import Foundation
import QuartzCore
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// Production source/profile worker handoffs on Metal for both Apple policies.
@main struct SourceEditOwnerChecks {
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
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-source-owner-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let imageURL = root.appendingPathComponent("Retained.png")
        let image = CGImage(width: 64, height: 48, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 256,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: CGDataProvider(data: Data(Array(repeating: [UInt8](arrayLiteral: 178,76,38,255), count: 64*48).flatMap {$0}) as CFData)!, decode: nil, shouldInterpolate: false, intent: .relativeColorimetric)!
        let output = CGImageDestinationCreateWithURL(imageURL as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(output, image, nil); try require(CGImageDestinationFinalize(output), "Write photo fixture")
        let photo = try Data(contentsOf: imageURL)
        let profileURL = root.appendingPathComponent("Display P3.icc")
        let bytes = CGColorSpace(name: CGColorSpace.displayP3)!.copyICCData()! as Data
        try bytes.write(to: profileURL)
        let invalid = root.appendingPathComponent("Broken.icc"); try Data("Invalid profile".utf8).write(to: invalid)
        let profile: JSON = try await withCheckedThrowingContinuation { done in
            NativeProjectTask.io.async {
                do {
                    do { _ = try ColorPreferencesStore(root: nil).importProfile(invalid); throw HostFailure(message: "Invalid ICC accepted") }
                    catch { if error.localizedDescription == "Invalid ICC accepted" { throw error } }
                    let profile = try ColorPreferencesStore(root: nil).importProfile(profileURL)
                    try require(profile["channels"].string == "Rgb", "ICC channels")
                    try require(Data(profile["profile"]["Icc"].array.map { UInt8($0.uint) }) == bytes, "Exact imported profile bytes")
                    done.resume(returning: profile)
                } catch { done.resume(throwing: error) }
            }
        }
        for platform: UInt32 in [0,1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root.appendingPathComponent("state-\(platform)")), managedWorkspaces: false)
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
            let saved=root.appendingPathComponent("Edited-\(platform).capy")
            store.projectFiles = ProjectFiles(store:store,dialogs:.init(open:{$0(imageURL)},save:{_,_,done in done(saved)},create:{_,done in
                done(JSON(["extent":[64,48],"color":["space":"Srgb","depth":"U8"],"background":"White"]))
            },paste:{$0(.success(photo))}))
            func invoke(_ command:String) async throws {try await edit(store,["type":"invoke","command":command])}
            func idle() async throws {
                try await wait("Document completion",store:store) {!store.projectFiles.busy && !store.state["requests"].array.contains {$0["kind"]["type"].string=="document"}}
                try require(store.projectFiles.error == nil,store.projectFiles.error ?? "")
            }
            func dialog(_ command:String) async throws -> DocumentColorController {
                try await invoke(command)
                try await wait("Source form",store:store) {store.projectFiles.colorEditor?.loaded == true || store.projectFiles.colorEditor?.error != nil}
                let editor=store.projectFiles.colorEditor!;try require(editor.error == nil,editor.error ?? "");return editor
            }
            func preview(_ editor:DocumentColorController,_ choice:JSON?) async throws {
                editor.prepare(choice)
                try await wait("Source comparison",store:store) {editor.ready || editor.error != nil}
                try require(editor.ready && editor.previews.count==2,editor.error ?? "Complete comparisons")
            }
            try await invoke("new_document");try await idle()
            try await invoke("paste_image");try await idle()
            let count=store.state["layers"].array.count
            let target=store.state["layer_tools"]["editing_layer"]["id"].uint
            var editor=try await dialog("repair_source_profile")
            try require(editor.source && !editor.sourceInfo["source_profile"].string.isEmpty,"Source details on worker")
            try await preview(editor,profile["profile"]);editor.cancel();try await idle()
            try require(store.state["layers"].array.count==count,"Cancel preserves layer count")
            editor=try await dialog("repair_source_profile")
            editor.prepare(JSON(["Icc":[]]))
            try await wait("Invalid profile",store:store) {editor.error != nil}
            try await preview(editor,profile["profile"])
            try require(!editor.sourceInfo["adds_layer"].bool,"Untouched photo repair replaces interpretation")
            editor.apply();try await idle();try require(store.state["layers"].array.count==count,"Untouched repair stays in place")
            try await invoke("undo");try await idle();try await invoke("redo");try await idle()
            try await invoke("select_all");try await edit(store,["type":"layer","action":["op":"fill_selection"]]);try await invoke("deselect")
            let prepared=await withCheckedContinuation {done in native.flushPersistence {done.resume(returning:$0)}};try require(prepared,"Commit paint before source repair")
            editor=try await dialog("repair_source_profile");try await preview(editor,JSON(["Builtin":"ProPhoto"]))
            try require(editor.sourceInfo["adds_layer"].bool,"Painted photo must preserve edits and add corrected original")
            editor.apply();try await idle();try require(store.state["layers"].array.count==count+1,"Corrected source layer added")
            try await invoke("undo");try await idle();try require(store.state["layers"].array.count==count,"One-step source Undo")
            try await invoke("redo");try await idle();try require(store.state["layers"].array.count==count+1,"One-step source Redo")
            try await edit(store,["type":"layer","action":["op":"select","id":target,"mask":false]])
            editor=try await dialog("rasterize_source");try await preview(editor,nil);editor.apply();try await idle()
            try require(!store.command("rasterize_source")["enabled"].bool,"Rasterized source cannot be rasterized again")
            try await invoke("undo");try await idle();try require(store.command("repair_source_profile")["enabled"].bool,"Undo restores retained original")
            editor=try await dialog("rasterize_source");editor.prepare(nil);editor.cancel();try await idle()
            try require(store.command("repair_source_profile")["enabled"].bool,"Worker cancellation preserves original")
            try await invoke("save_document");try await idle()
            try require(FileManager.default.fileExists(atPath:saved.path),"Save edited source stack")
            try require(try Data(contentsOf:imageURL)==photo && Data(contentsOf:profileURL)==bytes,"Never modify input photo or profile")
            print("PASS platform \(platform): ICC import/retry, complete source previews, Cancel/Apply, painted-source preservation, history, rasterization and worker cancellation")
        }
    }
}
