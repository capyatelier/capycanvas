import Foundation
import QuartzCore

/// Real Swift controller/worker ownership, without simulator or artist storage.
@main struct InspectionOwnerChecks {
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
            try require(Date() < deadline && store.failure == nil, store.failure ?? "Timed out: \(label); \(store.histogram.status)")
            let now = FrameTrace.now()
            await withCheckedContinuation { done in store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0,1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
            let native = store.native!
            let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            native.attach(surface, width: 128, height: 128, scale: 1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            try await wait("Metal startup", store: store) { store.snapshot["shaders_ready"].bool }
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) }, save: { _,_,done in done(nil) }, create: { _,done in
                done(JSON(["extent":[32,24],"color":["space":"DisplayP3","depth":"U16"],"background":"Transparent"]))
            }))
            try await edit(store, ["type":"invoke","command":"new_document"])
            try await wait("New drawing", store: store) { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            let model = store.histogram
            try await edit(store, ["type":"invoke","command":"histogram"])
            try await wait("Full histogram", store: store) { !model.result.isNull && !model.busy }
            try require(model.isOpen && model.result["histogram"]["pixels"].uint == 0, "Transparent pixels excluded")
            try require(model.result["histogram"]["transparent"].uint == 768, "Full resolution")
            try require(model.result["histogram"]["color"]["depth"].string == "U16", "Document precision")
            try require(!store.projectFiles.blocksEditor && store.state["requests"].array.isEmpty, "Nonmodal request acknowledged")
            let paper = store.state["layers"].array.first { $0["label"].string == "Paper" }!["id"].uint
            model.automatic = false
            try await edit(store, ["type":"layer","action":["op":"visibility","id":paper,"value":true]])
            try await wait("Revision change", store: store) { model.stale }
            try require(!model.busy && model.result["histogram"]["pixels"].uint == 0, "Manual inspection retains previous result")
            model.refresh()
            try await wait("Manual refresh", store: store) { !model.busy && !model.stale }
            try require(model.result["histogram"]["pixels"].uint == 768, "Visible paper counted")
            for channel in model.result["histogram"]["channels"].array {
                try require(channel["bins"][255].uint == 768, "White paper bins")
            }
            model.automatic = true
            try await edit(store, ["type":"invoke","command":"undo"])
            try await wait("Automatic refresh after Undo", store: store) { !model.busy && !model.stale && model.result["histogram"]["pixels"].uint == 0 }
            model.refresh(); model.close()
            await withCheckedContinuation { done in NativeProjectTask.io.async { done.resume() } }
            try await Task.sleep(for: .milliseconds(400))
            try require(!model.isOpen && !model.busy && model.result.isNull, "Closed inspector rejects late worker callbacks")
            try await edit(store, ["type":"invoke","command":"histogram"])
            try await wait("Reopen", store: store) { !model.result.isNull && !model.busy }
            try await edit(store, ["type":"invoke","command":"eyedropper"])
            for width in [1,5,15] {
                try await edit(store, ["type":"set_color_sample_size","width":width])
                try await wait("Sampling selection", store: store) {
                    store.state["color_picker"]["sample_width"].uint == UInt64(width)
                }
            }
            model.close()
            print("PASS Apple inspection owner policy \(platform): full resolution, manual/automatic updates, history, close/reopen and sampling controls")
        }
    }
}
