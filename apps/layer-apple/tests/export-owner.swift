import Foundation
import QuartzCore
import ImageIO
import UniformTypeIdentifiers

/// Real Metal owner and file coordinator, with disposable native-panel/provider
/// destinations. Library/preset operations run on the production file worker.
@main struct ExportOwnerChecks {
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    static func io<T>(_ work: @escaping () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { done in
            NativeProjectTask.io.async { done.resume(with: Result(catching: work)) }
        }
    }
    static func rejected(_ label: String, _ work: () throws -> Void) throws {
        var failed = false
        do { try work() } catch { failed = true }
        try require(failed, label)
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
    static func image(_ url: URL, type: UTType, depth: Int, extent: [Int]) throws {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
            let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
            let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] else {
            throw HostFailure(message: "ImageIO cannot reopen \(url.lastPathComponent)")
        }
        try require(CGImageSourceGetType(source) as String? == type.identifier, "\(url.lastPathComponent): expected \(type.identifier), got \(CGImageSourceGetType(source) as String? ?? "none")")
        try require(image.bitsPerComponent == depth && [image.width, image.height] == extent, "Output precision and dimensions")
        try require(properties[kCGImagePropertyProfileName as String] != nil && image.colorSpace?.copyICCData() != nil, "Embedded output profile")
    }
    static func library(_ root: URL) throws -> JSON {
        let preferences = ColorPreferencesStore(root: root.appendingPathComponent("preferences"))
        let color = JSON(["space": "ProPhoto", "depth": "U16"])
        try require(try preferences.profiles().isEmpty, "Missing library is empty")
        let initial = try preferences.presets(color: color, request: JSON(["type": "get", "index": 2]))
        try require(initial["recipe"]["depth"].string == "U16", "Missing preferences use shared defaults")
        let original = root.appendingPathComponent("Original.icc")
        let bytes = CGColorSpace(name: CGColorSpace.displayP3)!.copyICCData()! as Data
        try bytes.write(to: original)
        let profile = try preferences.importProfile(original)
        _ = try preferences.importProfile(original)
        let entries = try preferences.profiles()
        try require(entries.count == 1 && entries[0]["profile"].isNull, "Deduplicate profiles and keep list payloads small")
        let id = entries[0]["id"].string
        let selected = try preferences.profile(id)
        try require(Data(selected["profile"]["Icc"].array.map { UInt8($0.uint) }) == bytes, "Reuse exact ICC bytes")
        let recipe = initial["recipe"].replacing("profile", with: profile)
        let saved = try preferences.presets(color: color, request: JSON(["type": "save", "name": "Studio", "recipe": recipe.raw]))
        try require(saved["index"].uint == 4, "Save named recipe")
        let file = root.appendingPathComponent("preferences/color-export-presets.json")
        let savedBytes = try Data(contentsOf: file)
        try rejected("Reject duplicate preset") { _ = try preferences.presets(color: color,
            request: JSON(["type": "save", "name": "Studio", "recipe": recipe.raw])) }
        try rejected("Reject unsupported ICC before publication") {
            _ = try preferences.presets(color: color, request: JSON(["type": "update", "index": 4,
                "recipe": recipe.replacing("profile", with: JSON(["profile": ["Icc": [1,2,3]], "channels": "Rgb", "name": "Broken"])).raw]))
        }
        try require(try Data(contentsOf: file) == savedBytes, "Failed presets preserve the existing file")
        let stored = root.appendingPathComponent("preferences/color-profiles/\(id).icc")
        try Data("Damaged copy".utf8).write(to: stored)
        try require(try !preferences.profiles()[0]["issue"].isNull, "Report damaged saved profiles")
        try rejected("Reject changed saved profile") { _ = try preferences.profile(id) }
        _ = try preferences.importProfile(original)
        try require(try Data(contentsOf: stored) == bytes, "Reimport repairs library copy")
        try preferences.removeProfile(id)
        try require(try preferences.profiles().isEmpty && Data(contentsOf: original) == bytes, "Remove only the library copy")
        let reopened = ColorPreferencesStore(root: root.appendingPathComponent("preferences"))
        let restored = try reopened.presets(color: color, request: JSON(["type": "get", "index": 4]))
        try require(restored["recipe"].stableKey == recipe.stableKey, "Named preset retains ICC after library removal and restart")
        let update = recipe.replacing("format", with: JSON("Png"))
        _ = try reopened.presets(color: color, request: JSON(["type": "update", "index": 4, "recipe": update.raw]))
        _ = try reopened.presets(color: color, request: JSON(["type": "remember", "index": 1, "recipe": update.raw]))
        _ = try reopened.presets(color: color, request: JSON(["type": "reset", "index": 1]))
        let reset = try reopened.presets(color: color, request: JSON(["type": "get", "index": 1]))
        try require(reset["recipe"]["depth"].string == "U8", "Reset destination uses shared default")
        let removed = try reopened.presets(color: color, request: JSON(["type": "remove", "index": 4]))
        try require(removed["names"].array.count == 4, "Remove named preset")
        let overflowRoot = root.appendingPathComponent("overflow")
        let overflowDirectory = overflowRoot.appendingPathComponent("color-profiles")
        try FileManager.default.createDirectory(at: overflowDirectory, withIntermediateDirectories: true)
        let overflow = ColorPreferencesStore(root: overflowRoot)
        for index in 0..<129 {
            try Data([0]).write(to: overflowDirectory.appendingPathComponent(String(format: "%064x.icc", index)))
        }
        let overflowEntries = try overflow.profiles()
        try require(overflowEntries.count == 129 && overflowEntries.allSatisfy { !$0["issue"].isNull },
            "Every corrupt or over-quota entry must stay visible and removable")
        try rejected("Shared quota prevents publication") { _ = try overflow.importProfile(original) }
        for entry in overflowEntries.prefix(2) { try overflow.removeProfile(entry["id"].string) }
        _ = try overflow.importProfile(original)
        try require(try overflow.profiles().count == 128, "Removing excess entries permits import again")
        try rejected("Reject read outside the library") { _ = try overflow.profile("../Original") }
        try rejected("Reject removal outside the library") { try overflow.removeProfile("../Original") }
        try require(try Data(contentsOf: original) == bytes, "Rejected keys must preserve the original")
        print("PASS ICC library: exact import/reuse, deduplication, damaged-copy retry, safe removal, atomic named/destination presets")
        print("PASS shared ICC library: over-quota entries remain visible/removable, import resumes after cleanup and invalid keys preserve sources")
        return profile
    }
    @MainActor static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-export-owner-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let profile = try await io { try library(root) }
        let cmyk = try await io { try ColorPreferencesStore(root: nil).importProfile(
            URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")) }
        for platform: UInt32 in [0,1] {
            let directory = root.appendingPathComponent("state-\(platform)")
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: directory), managedWorkspaces: false)
            let native = store.native!
            let surface = CAMetalLayer(); surface.bounds = CGRect(x:0,y:0,width:128,height:128)
            native.attach(surface,width:128,height:128,scale:1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            let deadline = Date().addingTimeInterval(45)
            while !store.snapshot["shaders_ready"].bool {
                try require(Date() < deadline && store.failure == nil, store.failure ?? "Metal startup")
                let now = FrameTrace.now()
                await withCheckedContinuation { done in native.frame(now:now,target:now+16_666_667) { _,_,_ in done.resume() } }
                try await Task.sleep(for: .milliseconds(10))
            }
            let master = root.appendingPathComponent("Editable-\(platform).capy")
            var destination: URL? = master
            var chosenType: UTType?
            var staging: URL?
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { $0(nil) }, save: { _,type,done in
                chosenType = type; done(destination)
            }, create: { _,done in done(JSON(["extent": [64,48], "color": ["space": "DisplayP3", "depth": "U16"], "background": "Transparent"])) },
                export: platform == 0 ? { url,done in
                    staging = url
                    if let destination {
                        do { try FileManager.default.copyItem(at: url, to: destination); done(destination) }
                        catch { preconditionFailure("Fixture provider: \(error)") }
                    } else { done(nil) }
                } : nil, exportOptions: { _ in }))
            func invoke(_ command: String) async throws { try await edit(store, ["type": "invoke", "command": command]) }
            func idle(allowError: Bool = false) async throws {
                try await wait("Document completion", store: store) { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
                if !allowError { try require(store.projectFiles.error == nil, store.projectFiles.error ?? "") }
            }
            func dialog() async throws -> ExportController {
                try await invoke("export_document")
                try await wait("Export options", store: store) { store.projectFiles.exportEditor?.loaded == true || store.projectFiles.exportEditor?.error != nil }
                let editor = store.projectFiles.exportEditor!
                try require(editor.error == nil, editor.error ?? "")
                return editor
            }
            func finished(_ editor: ExportController) async throws {
                try await wait("Export worker", store: store) { !editor.busy }
                try require(editor.error == nil, editor.error ?? "")
            }
            try await invoke("new_document"); try await idle()
            try await edit(store, ["type":"color", "action":["op":"definition", "color":["space":"DisplayP3", "rgba":[0.7,0.3,0.15,0.8]]]])
            try await invoke("select_all"); try await edit(store, ["type":"layer", "action":["op":"fill_selection"]]); try await invoke("deselect")
            _ = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try await invoke("save_document"); try await idle()
            try await invoke("add_layer") // Export must preserve unsaved state too.
            let state = store.state["document_file"].stableKey
            let original = try Data(contentsOf: master)
            var editor = try await dialog()
            editor.cancel(); try await idle()
            try require(store.state["document_file"].stableKey == state, "Cancel leaves unsaved master unchanged")
            editor = try await dialog()
            editor.preference(JSON(["type": "get", "index": 2])); try await finished(editor)
            editor.change("format", JSON("Jpeg")); try await finished(editor)
            try require(editor.recipe["depth"].string == "U8" && editor.recipe["background"].string == "White"
                && editor.draft["depths"].array.map(\.string) == ["U8"]
                && !editor.draft["backgrounds"].array.map(\.string).contains("Preserve"), "JPEG draft supplies valid dependent values and choices")
            editor.change("encoding", editor.recipe["encoding"].replacing("dither", with: JSON("Stochastic8")))
            try await finished(editor)
            editor.change("format", JSON("Png")); try await finished(editor)
            editor.change("depth", JSON("U16")); try await finished(editor)
            try require(editor.recipe["encoding"]["dither"].string == "None"
                && editor.draft["dithers"].array.map(\.string) == ["None"], "16-bit draft removes inapplicable dither")
            editor.imported(cmyk); try await finished(editor)
            try require(editor.recipe["format"].string == "Tiff" && editor.recipe["background"].string == "White"
                && editor.draft["formats"].array.map(\.string) == ["Tiff", "Jpeg"]
                && !editor.draft["backgrounds"].array.map(\.string).contains("Preserve"), "CMYK import supplies supported output choices")
            let cmykOutput = root.appendingPathComponent("CMYK-\(platform).tiff")
            destination = cmykOutput; editor.choose(editor.recipe); try await idle()
            try image(cmykOutput, type: .tiff, depth: 16, extent: [64,48])
            let cmykSource = CGImageSourceCreateWithURL(cmykOutput as CFURL, nil)!
            try require(CGImageSourceCreateImageAtIndex(cmykSource, 0, nil)?.colorSpace?.model == .cmyk,
                "The shared draft must produce an actual CMYK delivery copy")
            editor = try await dialog()
            editor.preview(editor.recipe.replacing("jpeg_quality", with: JSON(0)))
            try await wait("Invalid output", store: store) { editor.error != nil }
            editor.preference(JSON(["type": "get", "index": 2])); try await finished(editor)
            editor.imported(profile); try await finished(editor)
            let recipe = editor.recipe.replacing("size", with: JSON(["Fit": ["bounds": [32,32], "enlarge": false]]))
                .replacing("resolution", with: JSON(["Ppi": 240]))
            editor.preview(recipe); try await finished(editor)
            try require(editor.previews.count == 2 && editor.details["output_extent"][0].uint == 32
                && editor.details["output_extent"][1].uint == 24, "Complete before/output comparison and fit")
            editor.preference(JSON(["type": "save", "name": "Delivery", "recipe": recipe.raw])); try await finished(editor)
            let preferences = store.colorPreferences
            let color = JSON(["space": "DisplayP3", "depth": "U16"])
            let previousCustom = try await io { try preferences.presets(color: color, request: JSON(["type":"get", "index":3])) }
            destination = nil; editor.choose(editor.recipe); try await idle()
            let remembered = try await io { try preferences.presets(color: color, request: JSON(["type":"get", "index":3])) }
            try require(remembered["recipe"].stableKey == previousCustom["recipe"].stableKey, "Cancelled destination preserves the previous Custom output choices")
            editor = try await dialog()
            editor.preference(JSON(["type":"get", "index":4])); try await finished(editor)
            try require(editor.recipe.stableKey == recipe.stableKey, "Reuse named output after cancel")
            let tiff = root.appendingPathComponent("Output-\(platform).tiff")
            destination = tiff; editor.choose(editor.recipe); try await idle()
            try image(tiff, type: .tiff, depth: 16, extent: [32,24])
            if platform == 1 { try require(chosenType == .tiff, "NSSavePanel receives TIFF type") }
            else {
                try await io { }
                try require(staging?.pathExtension == "tiff" && !FileManager.default.fileExists(atPath: staging!.path), "Provider staging type and cleanup")
            }
            let persisted = try await io { try ColorPreferencesStore(root: directory).presets(color: color, request: JSON(["type":"get", "index":3])) }
            try require(persisted["recipe"].stableKey == recipe.stableKey, "Successful named output remembers Custom, preserving named preset")
            for (format,type) in [("Png",UTType.png),("Jpeg",UTType.jpeg)] {
                editor = try await dialog()
                let output = root.appendingPathComponent("Output-\(platform)." + type.preferredFilenameExtension!)
                destination = output
                let value = editor.recipe.replacing("format", with: JSON(format)).replacing("background", with: JSON("White"))
                editor.choose(value); try await idle()
                try image(output, type:type, depth:8, extent:[64,48])
            }
            editor = try await dialog(); editor.preview(editor.recipe); editor.cancel(); try await idle()
            if platform == 1 {
                editor = try await dialog(); destination = root.appendingPathComponent("Missing/output.png")
                editor.choose(editor.recipe); try await idle(allowError:true)
                try require(store.projectFiles.error != nil, "Write failure is visible")
                store.projectFiles.error = nil
                editor = try await dialog(); destination = root.appendingPathComponent("Retry.png")
                editor.choose(editor.recipe.replacing("format", with: JSON("Png"))); try await idle()
                try image(destination!, type:.png, depth:8, extent:[64,48])
            }
            try require(store.state["document_file"].stableKey == state && (try Data(contentsOf:master)) == original,
                "All output operations preserve master bytes, location and dirty state")
            print("PASS platform \(platform): PNG/TIFF/JPEG, comparison, profile/resize/16-bit output, named presets, picker cancellation, retry and master preservation")
        }
    }
}
