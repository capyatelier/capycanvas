// Real layer list, AppKit contacts and shared Rust edits in one owned window.
import AppKit
import SwiftUI
import QuartzCore

@main final class LayerRowLifecycleChecks: NativeWorkspaceInputFixture {
    @MainActor static func edit(_ store: EditorStore, _ value: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            store.edit(value) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) }
                else { done.resume() }
            }
        }
        try await drain()
    }
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(10)
        while !ready() { try require(Date() < deadline, label); try await drain(0.01) }
    }
    @MainActor static func order(_ store: EditorStore) -> [UInt64] { store.state["layers"].array.map { $0["id"].uint } }
    @MainActor static func run(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Initial layers did not load") { order(store).count == 2 }
        for _ in 0..<22 {
            try await edit(store, ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]])
        }
        let initial = order(store), first = initial[0]
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 380, height: 260),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Layer list lifecycle check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let panel = JSON(["controls": [["control": "layers", "visible_in_panel": true]]])
        let host = NSHostingView(rootView: LayerPanel(store: store, panel: panel))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.contentView = nil; window.close() }
        try await drain(0.3)
        guard let marker = find(host), let model = marker.model as? LayerRowInteraction,
              let scroll = marker.enclosingScrollView else { throw HostFailure(message: "The layer list did not mount") }
        try await wait("Visible rows were not measured") { model.frames[first]?.row.height == 40 }
        func viewport() -> CGRect { marker.convert(scroll.contentView.bounds, from: scroll.contentView) }
        func body(_ id: UInt64) -> CGPoint { let frame = model.frames[id]!.name; return CGPoint(x: frame.midX, y: frame.midY) }
        var number = 1
        func send(_ type: NSEvent.EventType, _ point: CGPoint, pen: Bool = false) async throws {
            number += 1; try event(type, at: point, marker: marker, number: number, tablet: pen)
            try await drain(type == .leftMouseDown ? 0.02 : 0.08)
        }
        try require(initial.count == 24 && scroll.documentView!.bounds.height > viewport().height * 2,
            "The fixture must overflow the actual native scroll view")
        let start = body(first), origin = scroll.contentView.bounds.minY
        try await send(.leftMouseDown, start, pen: true)
        try await drain(try holdDuration(marker))
        try await wait("The held layer menu did not appear") { !model.menu.isNull }
        try await send(.leftMouseDragged, CGPoint(x: start.x, y: viewport().maxY - 10), pen: true)
        try await wait("Held layer edge scrolling did not retain its offscreen source") {
            scroll.contentView.bounds.minY > origin + 120 && model.contact.target?.id.hasSuffix(":layer:\(first)") == true
        }
        try require(model.contact.dragging && model.menu.isNull, "Scrolling the same pen contact must dismiss the menu")
        let drop = CGPoint(x: start.x, y: viewport().midY)
        try await send(.leftMouseDragged, drop, pen: true)
        guard let hint = model.drag, let target = hint.target else { throw HostFailure(message: "No measured layer drop hint after scrolling") }
        var expected = initial.filter { $0 != first }
        expected.insert(first, at: expected.firstIndex(of: target)! + (hint.fraction >= 0.5 ? 1 : 0))
        try await send(.leftMouseUp, drop, pen: true)
        try await wait("Scrolled layer drop did not match its indicated position") { order(store) == expected }
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(order(store) == initial, "One Undo must restore the offscreen row move")
        try await edit(store, ["type": "invoke", "command": "redo"])
        try require(order(store) == expected, "Redo must restore the same scrolled row move")
        note("PASS platform \(platform): native pen menu-to-drag, offscreen edge scrolling, indicated drop and Undo/Redo")

        scroll.contentView.scroll(to: .zero); scroll.reflectScrolledClipView(scroll.contentView); try await drain()
        let removed = order(store)[0]
        try await wait("Reset scroll did not measure its source") { model.frames[removed]?.row.intersects(viewport()) == true }
        let source = body(removed), destination = CGPoint(x: source.x, y: source.y + 60)
        try await send(.leftMouseDown, source)
        try await send(.leftMouseDragged, destination)
        try require(model.contact.dragging, "The removal check requires an active real mouse drag")
        try await edit(store, ["type": "layer", "action": ["op": "delete", "id": removed]])
        let afterRemoval = order(store)
        try await wait("Removing a dragged layer must retire capture without waiting for another pointer event") {
            model.contact.target == nil && model.drag == nil
        }
        try await send(.leftMouseUp, destination)
        try require(order(store) == afterRemoval && store.failure == nil, "Late release cannot commit or error after source removal")
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(order(store) == expected, "Undo after source removal must restore the deletion, with no cancelled drag entry")

        scroll.contentView.scroll(to: .zero); scroll.reflectScrolledClipView(scroll.contentView); try await drain()
        let remountedSource = order(store)[0]
        let remountedStart = body(remountedSource), remountedEnd = CGPoint(x: remountedStart.x, y: remountedStart.y + 60)
        try await send(.leftMouseDown, remountedStart)
        try await send(.leftMouseDragged, remountedEnd)
        try await edit(store, ["type": "layer", "action": ["op": "begin_rename", "id": remountedSource]])
        try await wait("Entering native name editing must retire the old drag") { model.contact.target == nil && model.drag == nil }
        try await send(.leftMouseUp, remountedEnd)
        try require(order(store) == expected, "A late release cannot reorder while the layer name is being edited")
        try await edit(store, ["type": "layer", "action": ["op": "cancel_rename"]])
        try await send(.leftMouseDown, remountedStart)
        try await send(.leftMouseDragged, remountedEnd)
        try require(model.contact.dragging, "The remount check requires an active drag")
        window.contentView = nil; try await drain()
        try require(model.contact.target == nil && model.drag == nil, "Removing the list must retire its drag")
        window.contentView = host; window.makeKeyAndOrderFront(nil); try await drain()
        try await send(.leftMouseUp, remountedEnd)
        try require(order(store) == expected && store.failure == nil, "A late release after remount cannot commit")
        note("PASS platform \(platform): active source removal/rename, late release, deletion Undo and list remount cancellation")

        // New documents can reuse numeric layer IDs. Capture must also retain
        // document identity across the reset.
        let reused = initial[initial.count - 2]
        scroll.contentView.scroll(to: CGPoint(x: 0, y: scroll.documentView!.bounds.height - scroll.contentView.bounds.height))
        scroll.reflectScrolledClipView(scroll.contentView); try await drain()
        try await wait("The document-reset source was not visible") {
            guard let frame = model.frames[reused]?.name, frame.width > 0 else { return false }
            return viewport().contains(CGPoint(x: frame.midX, y: frame.midY))
        }
        let resetStart = body(reused), resetEnd = CGPoint(x: resetStart.x, y: resetStart.y - 45)
        try await send(.leftMouseDown, resetStart) // Native admission refreshes the scrolled viewport.
        let epoch = store.state["document_file"]["epoch"].uint
        try await send(.leftMouseDragged, resetEnd)
        try require(model.contact.dragging, "Document reset must interrupt an active drag")
        // Document replacement uses the production Metal owner even though this
        // fixture presents only the layer panel and never captures the desktop.
        let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
        store.native!.attach(surface, width: 128, height: 128, scale: 1)
        let attached = await withCheckedContinuation { done in
            store.native!.submit(2, JSON(["type": "catalog"])) { done.resume(returning: $0 != nil) }
        }
        try require(attached && store.failure == nil, store.failure ?? "The document owner did not attach")
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { $0(nil) }, save: { _, _, completed in completed(nil) }, create: { _, completed in completed(JSON(["extent": [256, 256], "color": ["space": "Srgb", "depth": "U8"], "background": "White"])) }))
        store.invoke("new_document")
        try await wait("Document reset did not request the unsaved decision") { store.projectFiles.confirming }
        store.projectFiles.choose("discard")
        try await wait("Document reset did not complete") {
            store.state["document_file"]["epoch"].uint != epoch || store.projectFiles.error != nil
        }
        try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
        try require(order(store).contains(reused) && model.contact.target == nil && model.drag == nil,
            "A new document must cancel capture even when it reuses the source layer ID")
        let freshOrder = order(store)
        try await send(.leftMouseUp, resetEnd)
        try require(order(store) == freshOrder && store.failure == nil && store.projectFiles.error == nil,
            "Late release cannot edit the replacement document")
        note("PASS platform \(platform): document replacement cancels capture despite reused layer IDs")
    }
    @MainActor static func hierarchy(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Initial hierarchy did not load") { order(store).count == 2 }
        let paint = order(store)[0]
        try await edit(store, ["type": "layer", "action": ["op": "new", "group": true, "clipped": false]])
        let group = order(store)[0]
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 380, height: 260),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Layer hierarchy input check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let panel = JSON(["controls": [["control": "layers", "visible_in_panel": true]]])
        let host = NSHostingView(rootView: LayerPanel(store: store, panel: panel))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.contentView = nil; window.close() }
        try await drain(0.3)
        guard let marker = find(host), let model = marker.model as? LayerRowInteraction else {
            throw HostFailure(message: "Hierarchy row input did not mount")
        }
        try await wait("Hierarchy rows were not measured") { model.frames[paint]?.row.height == 40 }
        func row(_ id: UInt64) -> JSON { store.state["layers"].array.first { $0["id"].uint == id }! }
        var number = 400
        func drop(_ id: UInt64, on target: UInt64, fraction: CGFloat = 0.5) async throws {
            let source = model.frames[id]!.name, destination = model.frames[target]!.row
            let start = CGPoint(x: source.midX, y: source.midY)
            let end = CGPoint(x: source.midX, y: destination.minY + destination.height * fraction)
            number += 1; try event(.leftMouseDown, at: start, marker: marker, number: number); try await drain(0.02)
            number += 1; try event(.leftMouseDragged, at: end, marker: marker, number: number); try await drain()
            try require(model.contact.dragging && model.drag?.target == target, "Native hierarchy drag must reach its measured target")
            number += 1; try event(.leftMouseUp, at: end, marker: marker, number: number); try await drain()
        }
        try await drop(paint, on: group)
        try await wait("Dropping on a group center must reparent the layer") { row(paint)["depth"].uint == 1 }
        try require(!row(group)["collapsed"].bool, "A successful group drop must expose its child")
        let nestedOrder = order(store), nestedRevision = store.state["revision"].uint
        try await drop(group, on: paint)
        try await wait("Shared Rust must reject placing a group inside itself") { store.failure != nil }
        try require(order(store) == nestedOrder && store.state["revision"].uint == nestedRevision,
            "An invalid descendant drop cannot modify the document or history")
        store.failure = nil
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(row(paint)["depth"].uint == 0, "Undo after a rejected drop must restore the last successful reparent")

        for locked in [group, paint] {
            try await edit(store, ["type": "layer", "action": ["op": "lock", "id": locked, "value": true]])
            let before = store.state["revision"].uint
            try await drop(paint, on: group)
            try await wait("Shared Rust must reject a locked source or destination") { store.failure != nil }
            try require(store.failure!.localizedCaseInsensitiveContains("locked") && row(paint)["depth"].uint == 0
                && store.state["revision"].uint == before, "A locked drop must preserve document and hierarchy")
            store.failure = nil
            try await edit(store, ["type": "invoke", "command": "undo"])
            try require(!row(locked)["locked"].bool, "An invalid drop must not add a history entry after locking")
        }
        // The shared group interval includes 0.25 and excludes 0.75.
        try await drop(paint, on: group, fraction: 0.25)
        try await wait("The upper group boundary must reparent") { row(paint)["depth"].uint == 1 }
        try await drop(paint, on: group, fraction: 0.75)
        try await wait("The lower group boundary must insert outside the group") { row(paint)["depth"].uint == 0 }
        try require(store.failure == nil, store.failure ?? "")
        note("PASS platform \(platform): native group reparent/boundaries, locked source/destination and descendant rejection with history retained")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { for platform: UInt32 in [0, 1] { try await run(platform); try await hierarchy(platform) }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
