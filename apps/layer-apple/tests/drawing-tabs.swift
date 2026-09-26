import AppKit
import QuartzCore

@main struct DrawingTabChecks {
    @MainActor static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-tabs-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root.appendingPathComponent("owner-\(platform)")), managedWorkspaces: false)
            let native = store.native!, surface = CAMetalLayer()
            surface.bounds = CGRect(x: 0, y: 0, width: 256, height: 192)
            native.attach(surface, width: 256, height: 192, scale: 1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            func wait(_ name: String, _ ready: () -> Bool) async throws {
                let deadline = Date().addingTimeInterval(60)
                while !ready() {
                    if let error = store.failure ?? store.projectFiles.error { throw HostFailure(message: "\(name): \(error)") }
                    guard Date() < deadline else { throw HostFailure(message: "Timed out: \(name)") }
                    let now = FrameTrace.now()
                    await withCheckedContinuation { done in native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
                    try await Task.sleep(for: .milliseconds(10))
                }
            }
            func edit(_ action: [String: Any]) async throws {
                try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                    store.edit(action) { error in if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() } }
                }
            }
            func invoke(_ command: String) async throws {
                try await edit(["type": "invoke", "command": command])
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
                done(JSON(["extent": [96, 64], "color": ["space": "DisplayP3", "depth": "F32"], "background": "White"]))
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
            store.drawingTabs.confirmWindowClose { windowAllowed = $0 }
            try await wait("window close first prompt") { store.projectFiles.confirming }
            store.projectFiles.choose("discard")
            try await wait("window close second prompt") { store.projectFiles.confirming }
            store.projectFiles.choose("cancel")
            try await wait("window close cancelled") { windowAllowed != nil }
            precondition(windowAllowed == false && store.drawingTabs.rows.count == 3)
            // Cancelling any drawing revokes earlier approvals. All three dirty
            // owners must be prompted again on the next window-close attempt.
            windowAllowed = nil
            store.drawingTabs.confirmWindowClose { windowAllowed = $0 }
            for _ in ids {
                try await wait("window close reconsidered prompt") { store.projectFiles.confirming }
                store.projectFiles.choose("discard")
            }
            try await wait("window close approved") { windowAllowed != nil }
            precondition(windowAllowed == true && !store.drawingTabs.confirmingWindow)
            // Storage or native window/scene teardown can fail after every
            // drawing was approved. That rollback also revokes every approval.
            await withCheckedContinuation { done in store.cancelPreparedClose { done.resume() } }
            windowAllowed = nil
            store.drawingTabs.confirmWindowClose { windowAllowed = $0 }
            for _ in ids {
                try await wait("failed teardown reconsidered prompt") { store.projectFiles.confirming }
                store.projectFiles.choose("discard")
            }
            try await wait("failed teardown approval") { windowAllowed != nil }
            precondition(windowAllowed == true)
            print("Apple \(platform): device pickup matrix, independent history/save destinations, per-tab recovery, close/cancel, ordered multi-open and whole-window approval reset passed")
        }
    }
}
