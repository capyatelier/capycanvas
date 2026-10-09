import AppKit
import SwiftUI

private final class StartupErrors: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String] = []
    func receive(_ error: String?) {
        guard let error else { return }
        lock.lock(); values.append(error); lock.unlock()
    }
    var errors: [String] { lock.lock(); defer { lock.unlock() }; return values }
}

@main final class StartupOwnerChecks: NativeWorkspaceInputFixture {
    private static let cause = "Session launch: Unknown Apple platform \(UInt32.max)"

    @MainActor static func attribute(_ root: NSObject, _ name: String) -> Any? {
        let selector = NSSelectorFromString(name)
        return root.responds(to: selector) ? root.perform(selector)?.takeUnretainedValue() : nil
    }

    @MainActor static func element(_ root: NSObject, _ identifier: String) -> NSObject? {
        if attribute(root, "accessibilityIdentifier") as? String == identifier { return root }
        for child in attribute(root, "accessibilityChildren") as? [NSObject] ?? [] {
            if let found = element(child, identifier) { return found }
        }
        return nil
    }

    @MainActor static func enabled(_ element: NSObject) -> Bool {
        let selector = NSSelectorFromString("isAccessibilityEnabled")
        guard element.responds(to: selector) else { return false }
        let invoke = unsafeBitCast(element.method(for: selector), to: (@convention(c) (AnyObject, Selector) -> Bool).self)
        return invoke(element, selector)
    }

    @MainActor static func press(_ element: NSObject) -> Bool {
        let selector = NSSelectorFromString("accessibilityPerformPress")
        guard element.responds(to: selector) else { return false }
        let invoke = unsafeBitCast(element.method(for: selector), to: (@convention(c) (AnyObject, Selector) -> Bool).self)
        return invoke(element, selector)
    }

    @MainActor static func bounds(_ root: NSObject) -> CGRect {
        let selector = NSSelectorFromString("accessibilityFrame")
        guard root.responds(to: selector) else { return .zero }
        let invoke = unsafeBitCast(root.method(for: selector), to: (@convention(c) (AnyObject, Selector) -> CGRect).self)
        return invoke(root, selector)
    }

    @MainActor static func button(_ root: NSObject, label: String) -> NSObject? {
        if attribute(root, "accessibilityRole") as? String == "AXButton",
            attribute(root, "accessibilityLabel") as? String == label,
            attribute(root, "accessibilityIdentifier") as? String != "recovery-retry" { return root }
        return (attribute(root, "accessibilityChildren") as? [NSObject] ?? []).lazy.compactMap { button($0, label: label) }.first
    }

    @MainActor static func hierarchy(_ root: NSObject, depth: Int = 0) -> String {
        let frame = bounds(root)
        let row = "\(String(repeating: " ", count: depth))\(type(of: root)) \(attribute(root, "accessibilityRole") ?? "nil") \(attribute(root, "accessibilityIdentifier") ?? "nil") \(attribute(root, "accessibilityLabel") ?? "nil") frame=\(frame)"
        return ([row] + (attribute(root, "accessibilityChildren") as? [NSObject] ?? []).map { hierarchy($0, depth: depth + 1) }).joined(separator: "\n")
    }

    @MainActor static func capture(_ host: NSView, _ name: String) throws {
        note(hierarchy(host))
        guard let directory = ProcessInfo.processInfo.environment["CAPY_TEST_ARTIFACTS"],
            let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return }
        host.cacheDisplay(in: host.bounds, to: bitmap)
        try bitmap.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: directory).appendingPathComponent(name + ".png"))
    }

    @MainActor static func ownerChecks() async throws {
        let failures = StartupErrors()
        let owner = try NativeOwner(platform: UInt32.max, persistence: EditorPersistence(root: nil)) { _, error in failures.receive(error) }
        owner.submit(2, JSON(["type": "catalog"]))
        let layer = CAMetalLayer()
        owner.attach(layer, width: 64, height: 64, scale: 1)
        let result = await withCheckedContinuation { done in
            owner.frame(now: 1, target: 2) { again, revision, costs in done.resume(returning: (again, revision, costs)) }
        }
        try require(!failures.errors.isEmpty && failures.errors.allSatisfy { $0 == cause },
            "Queued startup operations replaced the original cause: \(failures.errors)")
        try require(!result.0 && result.1 == 0 && result.2.allSatisfy { $0 == 0 }, "A failed owner must return an idle frame")
        let before = failures.errors.count
        for _ in 0..<20 { await frame(owner) }
        try require(failures.errors.count == before, "A terminal launch failure must not report again on every frame")
        let restarted = await withCheckedContinuation { done in owner.restartCanvas { done.resume(returning: $0) } }
        try require(restarted == cause, "Restart must retry the real launch and retain its cause: \(restarted ?? "nil")")
        owner.detach()
        await frame(owner)
        try require(failures.errors.allSatisfy { $0 == cause }, "Detach replaced the original launch cause")
        note("PASS: real failed owner preserves launch cause through queued catalog, attach, frames, restart and detach")

        let store = EditorStore(platform: UInt32.max, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
        let driver = CanvasFrameDriver(store: store)
        var paused = true, wakes = 0
        driver.setPaused = { paused = $0 }
        store.wake = { wakes += 1; driver.wake() }
        driver.activate()
        try await wait("Original launch cause") { store.failure == cause && !store.bootstrap.isNull }
        let settledWakes = wakes
        for _ in 0..<20 {
            driver.tick(target: CACurrentMediaTime() + 1.0 / 60)
            await frame(store.native!)
            try await drain(0.01)
        }
        try require(paused && wakes == settledWakes && !store.canvasSubmitted,
            "Terminal startup must leave the native frame driver idle: paused=\(paused), wakes=\(wakes - settledWakes)")
        try require(store.canvasDiagnostic.hasPrefix(cause + "\n\nCapy Canvas "), "The user report lost its concrete launch cause")
        driver.deactivate()
        note("PASS: failed editor reports the concrete cause and returns its frame driver to idle")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-workspace-failure-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        let failedStore = EditorStore(platform: UInt32.max, persistence: EditorPersistence(root: root))
        let workspaces = failedStore.workspaces!
        try await wait("Workspace transport failure") { workspaces.error != nil }
        var flushed: Bool?, closed: Bool?, detached = false
        workspaces.flush { flushed = $0 }
        workspaces.close { closed = $0 }
        workspaces.detach { detached = true }
        try await wait("Failed workspace callbacks", seconds: 5) { flushed != nil && closed != nil && detached }
        try require(flushed == false && closed == false && workspaces.error?.contains(cause) == true,
            "Failed workspace transport must complete flush/close false and release the approved detach barrier")
        note("PASS: explicit workspace transport failure completes close, flush and approved teardown callbacks")
    }

    @MainActor static func viewChecks() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-startup-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        for theme in [ColorScheme.light, .dark] {
            let store = EditorStore(platform: UInt32.max, persistence: EditorPersistence(root: directory.appendingPathComponent("failed-\(theme)")), managedWorkspaces: false)
            try await wait("Launch failure report") { store.failure == cause && !store.bootstrap.isNull }
            try require(store.recovery.restoring, "The fixture must retain pending recovery at fatal startup")
            let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 900, height: 650),
                styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let host = NSHostingView(rootView: EditorView(store: store) { Color.clear }.environment(\.colorScheme, theme))
            window.contentView = host
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            defer { window.contentView = nil; window.close() }
            do { try await wait("Fatal startup Restart control") { element(host, "Restart Canvas") != nil } }
            catch { note(hierarchy(host)); throw error }
            let restart = element(host, "Restart Canvas")!
            try capture(host, "startup-failure-\(theme)")
            try require(enabled(restart), "Pending recovery disabled the fatal startup Restart control")
            try require(press(restart) && store.restartingCanvas,
                "The native Restart control must accept input while recovery remains pending")
            try await wait("Retried invalid launch") { !store.restartingCanvas }
            try require(store.failure == cause && store.recovery.restoring, "Failed retry must retain both launch cause and recovery state")
            try require(element(host, "canvas-error-detail") != nil && element(host, "Restart Canvas").map(enabled) == true,
                "The failed retry must leave its report and retry control usable")
            note("PASS \(theme): pending artwork recovery leaves fatal startup report and native Restart usable")
        }
        for theme in ["light", "dark"] {
            let root = directory.appendingPathComponent("warning-\(theme)")
            let failed = StorageLocations.within(root)!.sessions.appendingPathComponent("failed")
            let drawing = failed.appendingPathComponent("drawing")
            try FileManager.default.createDirectory(at: drawing, withIntermediateDirectories: true)
            let head = drawing.appendingPathComponent("head.json"), bytes = Data("unreadable session".utf8)
            try bytes.write(to: head)
            try Data(#"{"generation":1,"drawings":[{"id":1,"key":"drawing"}],"active":1,"clean_exit":false,"restoring":[],"blocked":[]}"#.utf8)
                .write(to: failed.appendingPathComponent("window.json"))
            let store = EditorStore(platform: 1, persistence: EditorPersistence(root: root), managedWorkspaces: true)
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) },
                save: { _, _, done in done(nil) }, paste: { done in done(.success([PhotoItem { _ in }])) }))
            let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 1000, height: 750),
                styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            let host = NSHostingView(rootView: EditorView(store: store) { MacMetalCanvas(store: store) })
            window.contentView = host
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            defer { window.contentView = nil; window.close() }
            try await wait("Unreadable session warning", seconds: 60, failure: { store.failure }) {
                !store.recovery.restoring && store.recovery.restoreError != nil && element(host, "recovery-retry") != nil
            }
            try await store.apply(["type": "set_theme", "theme": theme])
            store.workspaces!.send(["type": "focus_failed", "error": "Owned fixture workspace failure"])
            let workspaceRetry = store.catalog["native_copy"]["header"]["retry"].string
            try await wait("Workspace warning") {
                store.workspaces!.visibleError != nil && !store.workspaces!.presented && button(host, label: workspaceRetry) != nil
            }
            try await store.apply(["type": "invoke", "command": "paste_image"])
            let cancelLabel = store.bootstrap["common"]["cancel"].string
            try await wait("Pending provider Cancel") { store.projectFiles.busy && button(host, label: cancelLabel) != nil }
            try capture(host, "simultaneous-startup-warnings-\(theme)")
            let actions = [element(host, "recovery-retry")!, button(host, label: workspaceRetry)!, button(host, label: cancelLabel)!]
            for (index, action) in actions.enumerated() {
                let rect = bounds(action)
                try require(rect.width > 0 && rect.height > 0 && window.frame.contains(rect), "Every notice action must remain inside the native window")
                try require(actions.prefix(index).allSatisfy { !bounds($0).intersects(rect) }, "Simultaneous notice actions must not overlap")
            }
            try require(enabled(actions[2]) && press(actions[2]), "Recovery warnings must leave native document Cancel usable")
            try await wait("Provider cancellation completed") { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            note("PASS \(theme): workspace, document Cancel and recovery actions remain visible, separate and usable")
            let workspaceID = store.workspaces!.view["id"].string
            let workspaceOwner = store.workspaces!.view["owner"].string
            try require(press(button(host, label: workspaceRetry)!), "Native workspace Retry rejected input")
            try await wait("Retried existing workspace") { store.workspaces!.ready && !store.workspaces!.busy && store.workspaces!.error == nil }
            try require(store.workspaces!.view["id"].string == workspaceID && store.workspaces!.view["owner"].string == workspaceOwner,
                "Workspace Retry must preserve this window's existing workspace and owner")
            let original = store.recovery.restoreError
            let retry = element(host, "recovery-retry")!
            try require(enabled(retry), "Native recovery Retry must be enabled")
            try require(press(retry) && store.recovery.restoring,
                "Native recovery Retry must start one restoration")
            try await wait("Retry completed", seconds: 60) { !store.recovery.restoring }
            try require(store.recovery.visibleError != nil && store.recovery.restoreError == original,
                "An unsuccessful retry must preserve the concrete warning")
            try await wait("Recovery Later control") { element(host, "recovery-later") != nil }
            try require(press(element(host, "recovery-later")!), "Native Later control rejected input")
            try await wait("Warning dismissed") { store.recovery.visibleError == nil && element(host, "recovery-later") == nil }
            try require(store.recovery.restoreError == original && !store.recovery.restoring,
                "Later must dismiss the warning while retaining the failed recovery copy")
            store.recovery.retry()
            try await wait("Retry after Later", seconds: 60) { !store.recovery.restoring && store.recovery.visibleError != nil }
            let sessions = try FileManager.default.contentsOfDirectory(at: store.native!.sessions!, includingPropertiesForKeys: nil)
            let retained = sessions.map { $0.appendingPathComponent("drawing/head.json") }
                .first { FileManager.default.fileExists(atPath: $0.path) }
            try require(try retained.map { try Data(contentsOf: $0) } == bytes, "Retry and Later must preserve unreadable artwork bytes")
            note("PASS \(theme): native warning Retry, Later dismissal and retry after dismissal preserve the failed copy")
            let orphan = store.native!.sessions!.appendingPathComponent(store.sessionIdentity).appendingPathComponent("unreferenced-copy")
            try FileManager.default.createDirectory(at: orphan.appendingPathComponent("resources"), withIntermediateDirectories: true)
            try FileManager.default.createDirectory(at: orphan.appendingPathComponent("generations"), withIntermediateDirectories: true)
            try Data().write(to: orphan.appendingPathComponent(".lock"))
            let marker = Data("invalid".utf8), markerFile = orphan.appendingPathComponent(".retiring")
            try marker.write(to: markerFile)
            try await store.apply(["type": "invoke", "command": "add_layer"])
            let saved = await withCheckedContinuation { done in store.recovery.flush { done.resume(returning: $0) } }
            try require(saved && store.recovery.error?.contains("Invalid drawing retirement intent") == true,
                "A committed checkpoint must retain its cleanup warning without reporting save failure: saved=\(saved), error=\(store.recovery.error ?? "nil")")
            try await store.apply(["type": "invoke", "command": "add_layer"])
            try await wait("Autosave after cleanup warning", seconds: 20) { store.recovery.hasCurrentCopy }
            try require(try Data(contentsOf: markerFile) == marker, "A malformed orphan must remain intact after repeated checkpoints")
            note("PASS \(theme): committed cleanup warning does not block the next edited artwork autosave")
            try await prepare(store.native!, "Capture before blocked checkpoint")
            let blocked = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
            NativeProjectTask.io.async {
                blocked.signal()
                precondition(release.wait(timeout: .now() + 20) == .success, "The test must release its private file worker")
            }
            defer { release.signal() }
            let started = await Task.detached { blocked.wait(timeout: .now() + 5) == .success }.value
            try require(started, "The private checkpoint worker must be blocked before capture")
            var flushed: Bool?
            store.recovery.flush { flushed = $0 }
            await withCheckedContinuation { done in
                store.native!.submit(2, JSON(["type": "catalog"])) { _ in done.resume() }
            }
            try require(store.recovery.saving && flushed == nil, "The checkpoint must be queued behind the blocked file worker")
            try await store.apply(["type": "invoke", "command": "add_layer"])
            let newest = store.state["layers"].array.count
            release.signal()
            try await wait("Newest edit checkpoint acknowledgement", seconds: 15) { flushed != nil }
            try require(flushed == true && store.recovery.hasCurrentCopy,
                "A committed cleanup warning must not acknowledge a stale checkpoint after a newer edit")
            let scene = store.native!.sessions!.appendingPathComponent(store.sessionIdentity)
            let manifest = try JSON.decode(String(decoding: Data(contentsOf: scene.appendingPathComponent("window.json")), as: UTF8.self))
            let key = manifest["drawings"].array.first { $0["id"].uint == manifest["active"].uint }!["key"].string
            let savedDrawing = scene.appendingPathComponent(key)
            let checkpointHead = try JSON.decode(String(decoding: Data(contentsOf: savedDrawing.appendingPathComponent("head.json")), as: UTF8.self))
            let metadata = try JSON.decode(String(decoding: Data(contentsOf: savedDrawing.appendingPathComponent("generations/" + checkpointHead["current"]["id"].string + ".json")), as: UTF8.self))
            let objects = metadata["objects"].array
            let persisted = metadata["current"]["objects"].array.filter { objects[Int($0.uint)]["record"]["type"].string == "capy.occurrence/3" }.count
            try require(persisted == newest, "Successful flush must persist the newest layer state: saved=\(persisted), expected=\(newest)")
            note("PASS \(theme): cleanup-warning flush waits for an edit accepted after capture and persists its newest layer state")
        }
    }

    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await ownerChecks(); try await viewChecks(); exit(0) }
            catch { note("FAIL: \(error.localizedDescription)"); exit(1) }
        }
        NSApp.run()
    }
}
