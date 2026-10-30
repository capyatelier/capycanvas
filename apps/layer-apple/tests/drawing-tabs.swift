import AppKit
import QuartzCore

@main struct DrawingTabChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        try require(ProcessInfo.processInfo.environment["CAPY_STORAGE_DIR"] != nil && StorageLocations.installation != nil,
            "The fixture must configure its private process storage before temporary-file operations")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-tabs-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for platform: UInt32 in [0, 1] {
            let memory = EditorStore(platform: platform, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
            let native = memory.native!, surface = attachSurface(memory, CGSize(width: 256, height: 192))
            defer { native.detach(); withExtendedLifetime(surface) {} }
            try await CapyTest.wait("Memory-only startup", seconds: 60, step: { await frame(native) }) {
                memory.snapshot["shaders_ready"].bool && !memory.recovery.restoring
            }
            precondition(native.sessions == nil)
            for cleanExit in [false, true] {
                let flushed = await withCheckedContinuation { done in
                    memory.recovery.flush(cleanExit: cleanExit) { done.resume(returning: $0) }
                }
                precondition(flushed && !memory.recovery.saving && memory.recovery.error == nil)
            }
            let removed = await withCheckedContinuation { done in memory.recovery.removeDrawing(1) { done.resume(returning: $0) } }
            let closed = await withCheckedContinuation { done in memory.recovery.close { done.resume(returning: $0) } }
            precondition(removed && closed && !memory.recovery.saving)
            FileHandle.standardError.write(Data("Apple \(platform): memory-only checkpoint, removal and close callbacks completed\n".utf8))
        }
        for platform: UInt32 in [0, 1] {
            let directory = root.appendingPathComponent("unreadable-\(platform)")
            let session = StorageLocations.within(directory)!.sessions.appendingPathComponent("failed")
            let drawing = session.appendingPathComponent("drawing")
            try FileManager.default.createDirectory(at: drawing, withIntermediateDirectories: true)
            let head = drawing.appendingPathComponent("head.json"), bytes = Data("unreadable session".utf8)
            try bytes.write(to: head)
            try Data(#"{"generation":1,"drawings":[{"id":1,"key":"drawing"}],"active":1,"clean_exit":false,"restoring":[],"blocked":[]}"#.utf8)
                .write(to: session.appendingPathComponent("window.json"))
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: directory), managedWorkspaces: false)
            let native = store.native!, surface = attachSurface(store, CGSize(width: 256, height: 192))
            defer { native.detach(); withExtendedLifetime(surface) {} }
            try await CapyTest.wait("Unreadable session", seconds: 60, step: { await frame(native) }) {
                !store.recovery.restoring && store.recovery.restoreError != nil
            }
            precondition(store.recovery.visibleError != nil)
            try await store.apply(["type": "invoke", "command": "add_layer"])
            let saved = await withCheckedContinuation { done in store.recovery.flush { done.resume(returning: $0) } }
            precondition(saved && store.recovery.restoreError != nil)
            store.recovery.retry(); store.recovery.retry()
            try await CapyTest.wait("Retry unreadable session", seconds: 60, step: { await frame(native) }) { !store.recovery.restoring }
            precondition(store.recovery.visibleError != nil)
            store.recovery.later()
            precondition(store.recovery.visibleError == nil && store.recovery.restoreError != nil)
            let closed = await withCheckedContinuation { done in store.recovery.close { done.resume(returning: $0) } }
            precondition(closed && store.recovery.error == nil)
            let directories = try FileManager.default.contentsOfDirectory(at: native.sessions!, includingPropertiesForKeys: nil)
            let preserved = directories.first { FileManager.default.fileExists(atPath: $0.appendingPathComponent("drawing/head.json").path) }!
            let retained = try Data(contentsOf: preserved.appendingPathComponent("drawing/head.json"))
            precondition(retained == bytes)
            print("Apple \(platform): unreadable session, new checkpoint, retry, Later and close preserve the failed copy")
        }
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root.appendingPathComponent("owner-\(platform)")), managedWorkspaces: false)
            let native = store.native!, surface = attachSurface(store, CGSize(width: 960, height: 720))
            defer { native.detach(); withExtendedLifetime(surface) {} }
            func wait(_ name: String, _ ready: () -> Bool) async throws {
                try await CapyTest.wait(name, seconds: 60, failure: { (store.recovery.error ?? store.failure ?? store.projectFiles.error).map { "\(name): \($0)" } },
                    step: { await frame(native) }, ready)
            }
            func invoke(_ command: String) async throws {
                try await store.apply(["type": "invoke", "command": command])
                try await wait(command) { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            }
            func select(_ id: UInt64) async throws {
                let okay = await withCheckedContinuation { done in store.drawingTabs.select(id) { done.resume(returning: $0) } }
                guard okay else { throw HostFailure(message: store.projectFiles.error ?? "Tab activation failed") }
                try await wait("activation startup") { store.snapshot["shaders_ready"].bool }
                precondition(store.drawingTabs.selected == id)
            }
            try await wait("startup") { store.snapshot["shaders_ready"].bool }
            let a = root.appendingPathComponent("First-\(platform).capy"), b = root.appendingPathComponent("Second-\(platform).capy")
            var save = a
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) }, save: { _, _, done in done(save) }, create: { _, done in
                done(newSheet([96, 64], space: "DisplayP3", depth: "F32"))
            }))
            try await invoke("add_layer")
            let firstLayers = store.state["layers"].stableKey
            try await invoke("save_document_as")
            let firstBytes = try Data(contentsOf: a)
            try await invoke("new_document")
            precondition(store.drawingTabs.rows.count == 2 && store.drawingTabs.selected == 2)
            precondition(store.snapshot["proof_panel"]["depth"].string == "F32")
            precondition(store.drawingTabs.selectedDescription.hasSuffix(" · 96 × 64"))
            // Exercise the actual tab hit-target model with all native device
            // identities. Sensors and OS movement slop remain native-adapter work.
            let drag = DrawingTabInteraction()
            drag.controller = store.drawingTabs
            drag.viewport = CGRect(x: 0, y: 0, width: 300, height: 100)
            drag.frames = [1: DrawingFrame(body: CGRect(x: 0, y: 0, width: 300, height: 48),
                grip: CGRect(x: 0, y: 0, width: 24, height: 48), close: CGRect(x: 276, y: 0, width: 24, height: 48))]
            for vertical in [false, true] {
                drag.vertical = vertical
                for device in [ReorderDevice.mouse, .touch, .pen] {
                    for grip in [false, true] {
                        let point = CGPoint(x: grip ? 12 : 100, y: 20)
                        let source = drag.source(at: point)!
                        drag.contact.prepare(source, device: device, origin: point)
                        let held = vertical && !grip && device != .mouse
                        precondition(drag.contact.move(to: CGPoint(x: point.x + 12, y: 20)) == !held)
                        drag.cancel()
                        drag.contact.prepare(drag.source(at: point)!, device: device, origin: point)
                        drag.contact.recognizeHold()
                        precondition((drag.menu != nil) == (device != .mouse))
                        precondition(drag.contact.move(to: CGPoint(x: point.x + 12, y: 20)))
                        precondition(drag.menu == nil)
                        drag.cancel()
                        precondition(drag.dragged == nil && drag.menu == nil)
                    }
                }
                precondition(drag.source(at: CGPoint(x: 290, y: 20)) == nil, "Close is never a reorder target")
            }

            try await invoke("add_layer"); try await invoke("add_layer")
            save = b; try await invoke("save_document_as")
            let secondLayers = store.state["layers"].stableKey
            func settledDrawing(_ id: UInt64, _ name: String) async throws {
                try await wait(name) {
                    store.drawingTabs.selected == id && !store.drawingTabs.busy
                        && !store.projectFiles.busy && store.state["requests"].array.isEmpty
                        && store.snapshot["shaders_ready"].bool
                }
                precondition(store.state["layers"].stableKey == (id == 1 ? firstLayers : secondLayers))
            }
            func cycle(_ command: String, to id: UInt64) async throws {
                try await store.apply(["type": "invoke", "command": command])
                try await settledDrawing(id, command)
            }
            func key(_ name: String, to id: UInt64, command: Bool = false, shift: Bool = false, alt: Bool = false) async throws {
                for pressed in [true, false] {
                    store.input(["type": "key", "key": name, "pressed": pressed, "repeat": false,
                        "modifiers": ["command": command, "shift": shift, "alt": alt]])
                }
                await withCheckedContinuation { done in
                    native.submit(2, JSON(["type": "catalog"])) { _ in DispatchQueue.main.async { done.resume() } }
                }
                try await settledDrawing(id, "\(name) drawing cycling")
            }
            func restoreSettings(_ settings: [String: Any]) async throws {
                try await store.apply(["type": "restore_settings", "settings": settings])
                try await wait("shortcut settings") { store.state["requests"].array.isEmpty }
            }
            drag.vertical = false
            drag.frames = [1: DrawingFrame(body: CGRect(x: 0, y: 0, width: 140, height: 48)),
                2: DrawingFrame(body: CGRect(x: 140, y: 0, width: 140, height: 48))]
            let slidePress = CGPoint(x: 200, y: 20)
            drag.contact.prepare(drag.source(at: slidePress)!, device: .mouse, origin: slidePress)
            precondition(drag.contact.move(to: CGPoint(x: 90, y: 20)))
            try await wait("drawing slide preview") { drag.slide != nil }
            drag.contact.release(at: CGPoint(x: 90, y: 20))
            try await key("tab", to: 1, command: true)
            try await wait("slide reorder survives drawing switch") { store.drawingTabs.rows.first?["id"].uint == 2 }
            drag.cancel()
            try await select(2)
            store.drawingTabs.edit(["op": "step", "id": 2, "forward": true])
            try await wait("drawing order restored") { store.drawingTabs.rows.first?["id"].uint == 1 }
            for theme in ["light", "dark"] {
                try await store.apply(["type": "set_theme", "theme": theme])
                try await wait("cycling theme") { store.state["requests"].array.isEmpty }
                let original = store.state["settings"].object
                for _ in 0..<2 {
                    try await cycle("next_drawing", to: 1)
                    try await cycle("previous_drawing", to: 2)
                }
                var settings = original
                var shortcuts = settings["shortcuts"] as? [String: Any] ?? [:]
                for (id, name) in [("command.NextDrawing", "f6"), ("command.PreviousDrawing", "f7")] {
                    shortcuts[id] = [["key": name, "command": false, "shift": false, "alt": false]]
                }
                settings["shortcuts"] = shortcuts
                try await restoreSettings(settings)
                precondition(store.state["settings"]["shortcuts"]["command.NextDrawing"][0]["key"].string == "f6")
                precondition(store.state["settings"]["shortcuts"]["command.PreviousDrawing"][0]["key"].string == "f7")
                try await key("tab", to: 2, command: true)
                try await key("tab", to: 2, command: true, shift: true)
                try await key("f6", to: 1)
                try await key("f7", to: 2)
                shortcuts["command.NextDrawing"] = [] as [[String: Any]]
                shortcuts["command.PreviousDrawing"] = [] as [[String: Any]]
                settings["shortcuts"] = shortcuts
                try await restoreSettings(settings)
                try await key("f6", to: 2)
                try await key("f7", to: 2)
                try await restoreSettings(original)
                try await key("tab", to: 1, command: true)
                try await key("tab", to: 2, command: true, shift: true)
                for alternate in [false, true] {
                    try await invoke("hand")
                    try await key("pagedown", to: 1, command: !alternate, alt: alternate)
                    try await invoke("hand")
                    try await key("pageup", to: 2, command: !alternate, alt: alternate)
                }
                try await invoke("hand")
                let camera = store.state["camera"]
                try await key("pagedown", to: 2)
                try require(store.state["camera"]["translation"].stableKey != camera["translation"].stableKey,
                    "Plain PageDown must move the Hand camera: before=\(camera.stableKey), after=\(store.state["camera"].stableKey), surface=\(surface.bounds)")
                precondition(store.state["camera"]["zoom"].stableKey == camera["zoom"].stableKey)
                try await invoke("fit_canvas")
                print("Apple \(platform) \(theme): shared adjacent requests, custom/disabled/restored shortcuts and Hand page aliases passed")
            }
            try await select(1)
            precondition(store.state["layers"].stableKey == firstLayers)
            try await invoke("add_layer"); try await invoke("save_document")
            let savedFirst = try Data(contentsOf: a)
            precondition(savedFirst != firstBytes, "Save must retain the first tab's destination")
            try await select(2); precondition(store.state["layers"].stableKey == secondLayers)
            try await invoke("add_layer")
            let recovered = await withCheckedContinuation { done in store.recovery.flush { done.resume(returning: $0) } }
            precondition(recovered && store.recovery.hasCurrentCopy)
            store.drawingTabs.close(2)
            try await wait("close prompt") { store.projectFiles.confirming }
            store.projectFiles.choose("cancel")
            try await wait("close cancellation") { !store.projectFiles.busy }
            precondition(store.drawingTabs.rows.count == 2)
            store.drawingTabs.close(2)
            try await wait("second close prompt") { store.projectFiles.confirming }
            store.projectFiles.choose("discard")
            try await wait("close tab") { store.drawingTabs.rows.count == 1 && !store.drawingTabs.busy }
            precondition(store.drawingTabs.selected == 1)
            store.projectFiles.openItems([a, b].map(PhotoItem.init(fileURL:)))
            try await wait("ordered multi-file open") { store.drawingTabs.rows.count == 3 && !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            precondition(store.drawingTabs.rows[1]["title"].string == a.lastPathComponent)
            precondition(store.drawingTabs.rows[2]["title"].string == b.lastPathComponent)
            precondition(store.drawingTabs.view["parked_renderers"].uint == 0)
            let ids = store.drawingTabs.rows.map { $0["id"].uint }
            for id in ids { try await select(id); try await invoke("add_layer") }
            var windowAllowed: Bool?
            store.projectFiles.confirmClose { windowAllowed = $0 }
            try await wait("session close admitted") { windowAllowed != nil }
            precondition(windowAllowed == true && !store.projectFiles.confirming && store.drawingTabs.rows.count == 3)
            await withCheckedContinuation { done in store.cancelPreparedClose { done.resume() } }
            windowAllowed = nil
            store.projectFiles.confirmClose { windowAllowed = $0 }
            try await wait("session close after rollback") { windowAllowed != nil }
            precondition(windowAllowed == true && !store.projectFiles.confirming && store.drawingTabs.rows.count == 3)
            print("Apple \(platform): device pickup matrix, independent history/save destinations, per-tab recovery, close/cancel, ordered multi-open and session checkpoint close without per-drawing prompts passed")
        }
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root.appendingPathComponent("crop-switch-\(platform)")), managedWorkspaces: false)
            let native = store.native!, surface = attachSurface(store, CGSize(width: 960, height: 720))
            defer { native.detach(); withExtendedLifetime(surface) {} }
            func wait(_ name: String, _ ready: () -> Bool) async throws {
                try await CapyTest.wait(name, seconds: 8, failure: { store.recovery.error ?? store.failure ?? store.projectFiles.error },
                    step: { await frame(native) }, ready)
            }
            try await wait("crop switch startup") { store.snapshot["shaders_ready"].bool }
            let firstWidth = store.state["tabs"][0]["width"].uint
            let firstHeight = store.state["tabs"][0]["height"].uint
            let firstLayers = store.state["layers"].stableKey
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) }, save: { _, _, done in done(nil) },
                create: { _, done in done(newSheet([800, 600])) }))
            try await store.apply(["type": "invoke", "command": "new_document"])
            try await wait("colored second drawing") { store.drawingTabs.rows.count == 2 && store.drawingTabs.selected == 2 && store.snapshot["shaders_ready"].bool }
            for command in ["add_layer", "select_all", "fill_selection", "deselect", "pencil", "crop"] {
                try await store.apply(["type": "invoke", "command": command])
            }
            try await wait("crop width control") { store.state["tool_settings"].array.contains { $0["id"].string == "crop_width" } }
            try await store.apply(["type": "set_tool_setting", "id": "crop_width", "value": 480])
            try await wait("modified crop") { store.state["tool_settings"].array.contains { $0["id"].string == "crop_width" && $0["value"].number == 480 } }
            var switched: Bool?
            store.drawingTabs.select(1) { switched = $0 }
            try await wait("modified crop drawing switch") { switched != nil }
            try require(switched == true && store.drawingTabs.selected == 1, "Switching away from a modified crop must finish")
            switched = nil
            store.drawingTabs.select(2) { switched = $0 }
            try await wait("cropped drawing return") { switched != nil && store.snapshot["shaders_ready"].bool }
            try require(switched == true && store.state["tabs"][0]["width"].uint == 480
                && store.state["tabs"][0]["height"].uint == 600 && store.state["brush"]["preset"].uint == 2
                && !store.state["tool_settings"].array.contains { $0["id"].string == "crop_width" },
                "Switching back must show the applied 480 × 600 crop and restore Pencil")
            print("Apple \(platform): modified crop commits before drawing switch and restores Pencil")
            for (mode, setting, value) in [("crop", "crop_width", 320), ("scale_rotate", "transform_x", 48)] {
                if mode == "scale_rotate" {
                    try await store.apply(["type": "invoke", "command": "new_document"])
                    try await wait("transform close drawing") { store.drawingTabs.rows.count == 2 && store.drawingTabs.selected != 1 && store.snapshot["shaders_ready"].bool }
                    for command in ["add_layer", "select_all", "fill_selection", "deselect", "pencil"] {
                        try await store.apply(["type": "invoke", "command": command])
                    }
                }
                try await store.apply(["type": "invoke", "command": mode])
                try await wait("\(mode) close control") { store.state["tool_settings"].array.contains { $0["id"].string == setting } }
                try await store.apply(["type": "set_tool_setting", "id": setting, "value": value])
                try await wait("modified \(mode) before close") { store.state["tool_settings"].array.contains { $0["id"].string == setting && $0["value"].number == Double(value) } }
                let closingID = store.drawingTabs.selected
                store.drawingTabs.close(closingID)
                try await wait("\(mode) close prompt") { store.projectFiles.confirming }
                store.projectFiles.choose("discard")
                try await wait("\(mode) drawing closed") { store.drawingTabs.rows.count == 1 && store.drawingTabs.selected == 1
                    && !store.drawingTabs.busy && !store.projectFiles.busy && store.snapshot["shaders_ready"].bool }
                try require(store.state["tabs"][0]["width"].uint == firstWidth && store.state["tabs"][0]["height"].uint == firstHeight
                    && store.state["layers"].stableKey == firstLayers
                    && !store.state["tool_settings"].array.contains { $0["id"].string == setting },
                    "Closing a drawing in \(mode) must preserve the surviving drawing and retire its mode")
                print("Apple \(platform): modified \(mode) closes and preserves the surviving drawing")
            }
        }
    }
}
