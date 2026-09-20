import AppKit
import SwiftUI

/// Real Metal owner, worker, durable profile storage and native default action.
/// All files and windows belong to this fixture; no artist preferences are read.
@main final class ProofOwnerChecks: NativeWorkspaceInputFixture {
    static func io<T>(_ work: @escaping () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { done in
            NativeProjectTask.io.async { done.resume(with: Result(catching: work)) }
        }
    }
    @MainActor static func run() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-proof-\(UUID())")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root), managedWorkspaces: false)
            let native = store.native!, model = store.proof, preferences = store.colorPreferences
            let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            native.attach(surface, width: 128, height: 128, scale: 1)
            defer { model.setPaused(true); native.detach(); withExtendedLifetime(surface) {} }
            func wait(_ label: String, _ ready: () -> Bool) async throws {
                let deadline = Date().addingTimeInterval(60)
                while !ready() {
                    try require(Date() < deadline && store.failure == nil, store.failure ?? "Timed out: \(label), \(model.error ?? "")")
                    let now = FrameTrace.now()
                    await withCheckedContinuation { done in native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
                    try await drain(0.01)
                }
            }
            func edit(_ action: [String: Any]) async throws {
                try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                    store.edit(action) { failure in
                        if let failure { done.resume(throwing: HostFailure(message: failure)) } else { done.resume() }
                    }
                }
            }
            func invoke(_ command: String) async throws { try await edit(["type": "invoke", "command": command]) }
            func query(_ type: String) async -> JSON {
                await withCheckedContinuation { done in store.query(["type": type]) { done.resume(returning: $0) } }
            }
            func idle() async throws {
                try await wait("Idle document") { !store.projectFiles.busy && store.state["requests"].array.isEmpty && !model.busy }
                try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            }
            func setup(_ command: String = "soft_proof_setup") async throws {
                try await invoke(command)
                try await wait("Proof panel") { model.setupID == nil && !model.form.isNull && !store.state["requests"].array.contains(where: { $0["kind"]["type"].string == "soft_proof_setup" }) }
            }
            func apply(_ recipe: JSON) async throws {
                model.apply(recipe)
                try await wait("Prepared proof") { !model.busy }
                try require(model.error == nil && model.setupID == nil, model.error ?? "Proof Setup must close after Apply")
                try await wait("Proof display") { model.status.hasPrefix("Proof:") }
            }
            try await wait("Metal startup") { store.snapshot["shaders_ready"].bool }
            let master = root.appendingPathComponent("Proof.capy")
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([master]) }, save: { _, _, done in done(master) }, create: { _, done in
                done(JSON(["extent": [64, 48], "color": ["space": "DisplayP3", "depth": "U16"], "background": "White"]))
            }))
            try await invoke("new_document"); try await idle()
            try await edit(["type": "color", "action": ["op": "definition", "color": ["space": "DisplayP3", "rgba": [0.8, 0.2, 0.1, 1.0]]]])
            try await invoke("select_all"); try await edit(["type": "layer", "action": ["op": "fill_selection"]]); try await invoke("deselect")
            let flushed = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try require(flushed, "Paint is complete")
            try await invoke("save_document"); try await idle()
            let original = try Data(contentsOf: master)
            try await setup("soft_proof")
            try require(!store.state["soft_proof"].bool, "First-use setup does not enable proof before Apply")
            model.close(); try await idle()
            try require(!store.state["document_file"]["modified"].bool && (try Data(contentsOf: master)) == original, "Cancel preserves saved drawing")
            try await setup()
            let base = model.form["recipe"]
            model.apply(base.replacing("profile", with: JSON(["Icc": [1, 2, 3]])))
            try await wait("Invalid profile") { !model.busy }
            try require(model.error != nil && model.setupID != nil, "Invalid profile remains retryable in Setup")
            model.apply(base); model.close(); try await idle(); try await drain(0.2)
            try require(!store.state["document_file"]["modified"].bool, "Cancel before worker capture rejects its late reply")

            let icc = CGColorSpace(name: CGColorSpace.displayP3)!.copyICCData()! as Data
            let embedded = try await io { try ColorPreferencesStore(root: nil).importProfile(icc) }
            try await setup()
            try await apply(base.replacing("name", with: JSON("Embedded P3")).replacing("profile", with: embedded["profile"]))
            try require(try await io { try preferences.profiles().isEmpty }, "Selecting embedded bytes does not require a library copy yet")
            try await setup()
            let srgb = base.replacing("name", with: JSON("sRGB proof")).replacing("profile", with: JSON(["Builtin": "Srgb"]))
            try await apply(srgb)
            let entries = try await io { try preferences.profiles() }
            try require(entries.count == 1, "Replacing embedded profile preserves one exact copy")
            let id = entries[0]["id"].string
            let preserved = root.appendingPathComponent("color-profiles/\(id).icc")
            try require(try Data(contentsOf: preserved) == icc, "Durable copy must preserve all ICC bytes")
            try await io { try preferences.showProfile(id, visible: false) }
            try require(try await io { try ColorPreferencesStore(root: root).profiles()[0]["visible"].bool } == false, "Visibility survives a fresh preferences store")
            try await io { try preferences.showProfile(id, visible: true) }
            try require(try await io { try preferences.profiles()[0]["visible"].bool }, "Hidden profile remains reversible")
            try await invoke("undo")
            try require(await query("proof_form")["recipe"]["name"].string == "Embedded P3", "Undo restores original proof recipe")
            try await invoke("redo")
            try require(await query("proof_form")["recipe"]["name"].string == "sRGB proof", "Redo restores replacement recipe")
            try await invoke("save_document"); try await idle()
            let proofFile = try Data(contentsOf: master)
            for command in ["gamut_warning", "soft_proof", "gamut_warning", "soft_proof"] { try await invoke(command) }
            try await idle()
            try require(!store.state["document_file"]["modified"].bool && (try Data(contentsOf: master)) == proofFile, "Viewing flags do not edit the saved drawing")
            model.setPaused(true)
            try await invoke("open_document"); try await idle()
            try require(await query("proof_form")["recipe"]["name"].string == "sRGB proof", "Save/reopen retains proof recipe")
            if !store.state["soft_proof"].bool { try await invoke("soft_proof") }
            try require(!model.busy, "A paused scene does not start proof work")
            model.setPaused(false)
            try await wait("Resumed proof") { model.status == "Proof: sRGB proof" && !model.busy }
            try require(await query("proof_status")["needed"].bool == false, "Reopened view has a prepared LUT")

            // Exercise the real SwiftUI default action, including picker layout.
            try await setup()
            let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 520, height: 460), styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let host = NSHostingView(rootView: ProofPanel(store: store, controller: model))
            window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            try await drain(0.3)
            if let directory = ProcessInfo.processInfo.environment["CAPY_PROOF_CAPTURE"] {
                // Native menu/button materials do not fully render into an
                // NSView cache bitmap. Capture this fixture window only.
                let capture = Process(); capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                capture.arguments = ["-x", "-o", "-l", String(window.windowNumber),
                    URL(fileURLWithPath: directory).appendingPathComponent("proof-form-\(platform).png").path]
                try capture.run(); capture.waitUntilExit()
                try require(capture.terminationStatus == 0, "Capture the owned Proof Setup window")
            }
            model.applyLive(srgb)
            try await wait("Live panel apply") { model.setupID == nil && !model.busy }
            try require(model.error == nil, model.error ?? "Live panel apply failed")
            window.contentView = nil; window.close()
            try await io { try preferences.removeProfile(id) }
            try require(try await io { try preferences.profiles().isEmpty }, "Removing a saved copy leaves embedded recipes independent")
            try require(await query("proof_form")["recipe"]["name"].string == "sRGB proof", "Library removal must not edit the drawing")
            let cmyk = try await io { try ColorPreferencesStore(root: nil).importProfile(
                URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")) }
            try require(cmyk["channels"].string == "Cmyk", "Exercise an actual print profile")
            try await setup()
            try await apply(srgb.replacing("name", with: JSON("CMYK paper proof")).replacing("profile", with: cmyk["profile"])
                .replacing("simulate_paper", with: JSON(true)))
            try await invoke("undo")
            try require(await query("proof_form")["recipe"]["name"].string == "sRGB proof", "CMYK setup is one reversible history step")
            note("PASS platform \(platform): RGB/CMYK proof preparation, retry, cancellation, exact ICC preservation, visibility, history, save/reopen, scene pause/resume and live panel edits")
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) } catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
