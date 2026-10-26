import Foundation
import QuartzCore

/// Production document coordinator, owner queues and durable palette/settings
/// storage in disposable sessions. UI delivery is a separate XCTest workflow.
@main struct ColorWorkflowChecks {
    @MainActor static func main() async throws {
        try await Task.detached {
            for language in ["en", "ja"] {
                let configuration = try JSON(["saved": "", "preferred_languages": [language]]).encoded()
                var error: UInt64 = 0
                let prepared = configuration.withCString { nativeHandle(capy_apple_prepare_launch($0, &error)) }
                let failure = takeNativeText(error)
                guard let prepared else { throw HostFailure(message: failure ?? "Profile language preparation failed") }
                capy_apple_launch_free(prepared)
            }
        }.value
        try profileProjectionAndCache()
        if CommandLine.arguments.contains("--profiles-only") {
            print("PASS: shared profile query projection, visibility identity and cache invalidation")
            return
        }
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-color-workflows-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let scene = UUID().uuidString
            let store = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
            try await wait("Workspace startup") { store.workspaces?.ready == true || store.failure != nil }
            try require(store.failure == nil, store.failure ?? "")
            let surface = attachSurface(store, CGSize(width: 128, height: 128))
            try await wait("Metal startup", failure: { store.failure }, step: {
                try await prepare(store.native!, "Metal startup")
                await frame(store.native!)
            }) { store.snapshot["shaders_ready"].bool }
            let files = store.projectFiles
            let epoch = store.state["document_file"]["epoch"].uint
            try await store.apply(["type": "invoke", "command": "new_document"])
            try await wait("New drawing dialog") { files.creating && !store.state["new_drawing"].isNull }
            for step in newSheet([63, 47], space: "DisplayP3", depth: "U16", transparent: true) { try await files.newDrawingStep(step) }
            for step: [String: Any] in [["op": "presets"], ["op": "save_as"], ["op": "preset_name", "text": " "]] { try await files.newDrawingStep(step) }
            try require(!store.state["new_drawing"]["drawer"]["save"]["can_save"].bool, "A blank preset name cannot be saved")
            var refused = false
            do { try await files.newDrawingStep(["op": "save_preset"]) } catch { refused = true }
            try require(refused && files.creating && store.state["document_file"]["epoch"].uint == epoch
                && store.state["settings"]["new_document"]["setups"].array.isEmpty, "A refused preset must retain the dialog, drawing and presets")
            for step: [String: Any] in [["op": "preset_name", "text": " Studio P3 "], ["op": "save_preset"]] { try await files.newDrawingStep(step) }
            try require(store.state["settings"]["new_document"]["setups"][0]["name"].string == "Studio P3", "Save the trimmed preset name")
            for step: [String: Any] in [["op": "use_preset"], ["op": "create"]] { try await files.newDrawingStep(step) }
            try await wait("New drawing") { store.state["document_file"]["epoch"].uint == epoch + 1 && !files.busy }
            try require(store.state["settings"]["new_document"]["recent"][0]["id"].string == "Studio P3", "Create remembers the preset it used")
            try require(store.state["colors"]["rgb_space"].string == "DisplayP3", "New must adopt the selected working space")
            try await store.apply(["type": "invoke", "command": "new_document"])
            try await wait("Reopened dialog") { files.creating && !store.state["new_drawing"].isNull }
            try require(store.state["new_drawing"]["page"]["preset"].string == "Studio P3", "Reopen with the last preset")
            try await files.newDrawingStep(["op": "cancel"])
            try await wait("New cancellation") { !files.busy }
            try require(store.state["document_file"]["epoch"].uint == epoch + 1, "Cancel must preserve the drawing")

            func color(_ action: [String: Any]) async throws { try await store.apply(["type": "color", "action": action]) }
            func library(_ action: [String: Any]) async throws { try await color(["op": "library", "action": action]) }
            let starting = store.state["color_library"]["palettes"].array.count
            try await library(["op": "create_palette", "name": "Studio colors"])
            let palette = store.state["color_library"]["palettes"].array.first { $0["name"].string == "Studio colors" }!["id"].uint
            func saved(_ owner: EditorStore) -> JSON {
                owner.state["color_library"]["palettes"].array.first { $0["id"].uint == palette } ?? JSON()
            }
            for space in ["Srgb", "DisplayP3", "AdobeRgb", "ProPhoto"] {
                let definition = JSON(["space": space, "rgba": [1.125, -0.125, 0.34567891, 213.0 / 65535]])
                try await color(["op": "set_slot", "slot": "foreground", "color": definition.raw])
                let tagged = store.state["colors"]["foreground"]
                try await library(["op": "store", "palette": palette, "name": space, "color": tagged.raw])
                let swatch = saved(store)["swatches"].array.last!
                try require(swatch["color"].stableKey == tagged.stableKey, "Store must retain extended values and precise alpha in \(space)")
                try await library(["op": "rename", "id": swatch["id"].uint, "name": "Saved " + space])
                try await color(["op": "set_slot", "slot": "foreground", "color": ["space": "Srgb", "rgba": [0, 0, 0, 1]]])
                try await library(["op": "use", "id": swatch["id"].uint])
                try require(store.state["colors"]["foreground"].stableKey == tagged.stableKey, "Use must restore exact tagged values in \(space)")
            }
            try await library(["op": "rename_palette", "id": palette, "name": "Retained colors"])
            let savedLibrary = store.state["color_library"].stableKey
            // A new document changes wheel space but retains workspace palettes.
            try await store.apply(["type": "invoke", "command": "new_document"])
            try await wait("Next document dialog") { files.creating && !store.state["new_drawing"].isNull }
            for step in newSheet([67, 43], space: "ProPhoto", depth: "U16") + [["op": "create"]] { try await files.newDrawingStep(step) }
            try await wait("Next document") { store.state["document_file"]["epoch"].uint == epoch + 2 && !files.busy }
            try require(store.state["color_library"].stableKey == savedLibrary && store.state["colors"]["rgb_space"].string == "ProPhoto",
                "Document adoption must preserve palettes while updating the wheel gamut")
            let settings = store.state["settings"]["new_document"].stableKey
            let flushed = await withCheckedContinuation { done in store.flushPersistence { done.resume(returning: $0) } }
            try require(flushed, "Settings and palettes must flush durably")
            try await store.workspaces!.closed()
            let restored = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
            try await wait("Fresh owner restoration") { restored.workspaces?.ready == true || restored.failure != nil }
            try require(restored.state["color_library"].stableKey == savedLibrary, "Fresh owner must restore palette IDs, names, tags and samples")
            try require(restored.state["settings"]["new_document"].stableKey == settings, "Fresh owner must restore New drawing presets")
            let removal = saved(restored)["swatches"][0]["id"].uint
            for action: [String: Any] in [["op": "remove", "id": removal], ["op": "remove_palette", "id": palette]] {
                try await restored.apply(["type": "color", "action": ["op": "library", "action": action]])
            }
            try require(restored.state["color_library"]["palettes"].array.count == starting, "Swatch and palette removal must preserve the remaining palettes")
            try await restored.workspaces!.closed()
            print("PASS: platform \(platform), New drawing presets/validation/Cancel, all tagged palette spaces, document adoption and fresh-owner durable restoration")
            withExtendedLifetime(surface) {}
        }
    }
    @MainActor static func profileProjectionAndCache() throws {
        let ids = ["a", "b", "c"].map { String(repeating: $0, count: 64) }
        let entries = [
            JSON(["id": ids[0], "bytes": 2602, "name": "", "channels": "Gray", "visible": false]),
            JSON(["id": ids[1], "bytes": 3101, "name": "Größe", "channels": "Rgb", "visible": true]),
            JSON(["id": ids[2], "bytes": 17, "name": "", "visible": false]),
        ]
        let original = entries.map(\.stableKey)
        let response = ColorUI.resolve(["type": "profile_entries", "entries": entries.map(\.raw), "query": "GRÖSSE"], language: "en")
        try require(response["error"].isNull, "Profile transport must accept metadata and prepared language: \(response.stableKey)")
        let filtered = ColorUI.profileEntries(entries, language: "en", query: "GRÖSSE")
        try require(filtered.count == 1 && filtered[0]["id"].string == ids[1] && filtered[0]["visible"].bool,
            "A filtered profile must retain visibility by ID rather than the input row position")
        for (name, query) in [("Işık İçe aktar", "IŞIK İÇE AKTAR"), ("Tiếng Việt", "Tie\u{302}\u{301}ng Vie\u{323}\u{302}t")] {
            let named = entries[1].replacing("name", with: JSON(name))
            let rows = ColorUI.profileEntries([entries[0], named], language: "en", query: query)
            try require(rows.count == 1 && rows[0]["name"].string == name && rows[0]["visible"].bool,
                "Native profile queries must use shared locale normalization and preserve literal names")
        }
        let cache = ProfileEntryCopyCache()
        let all = cache.rows(entries, language: "en")
        try require(Set(all.map { $0["id"].string }) == Set(ids), "An omitted profile query must include all metadata")
        try require(cache.rows(entries, language: "en", query: ids[1]).map { $0["id"].string } == [ids[1]],
            "A query change must invalidate retained profile captions")
        let unavailable = cache.rows(entries, language: "en", query: ids[2])
        try require(unavailable.count == 1 && unavailable[0]["id"].string == ids[2] && !unavailable[0]["visible"].bool,
            "Changing only the query must replace the filtered row and its visibility")
        try require(cache.rows(entries, language: "en", query: "no matching profile").isEmpty,
            "An unmatched query must not retain previous rows")
        try require(cache.rows(entries, language: "en").map(\.stableKey) == all.map(\.stableKey),
            "Clearing a query must restore every retained profile")
        let japanese = cache.rows(entries, language: "ja")
        let englishDetails = all.first { $0["id"].string == ids[0] }?["details"].string
        let japaneseDetails = japanese.first { $0["id"].string == ids[0] }?["details"].string
        try require(japanese.map(\.stableKey) == ColorUI.profileEntries(entries, language: "ja").map(\.stableKey)
            && japaneseDetails != englishDetails,
            "A language change must reproject retained captions without profile bytes")
        _ = cache.rows(entries, language: "ja", query: ids[1])
        var hidden = entries
        hidden[1] = hidden[1].replacing("visible", with: JSON(false))
        let changed = cache.rows(hidden, language: "ja", query: ids[1])
        try require(changed.count == 1 && !changed[0]["visible"].bool, "Metadata changes must invalidate cached visibility")
        try require(entries.map(\.stableKey) == original, "Projection and filtering must preserve retained metadata")
    }
}
