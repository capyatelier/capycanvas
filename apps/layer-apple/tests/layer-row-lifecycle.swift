// Real layer list, AppKit contacts and shared Rust edits in one owned window.
import AppKit
import SwiftUI
import QuartzCore

@main final class LayerRowLifecycleChecks: NativeWorkspaceInputFixture {
    @MainActor static func edit(_ store: EditorStore, _ value: [String: Any]) async throws {
        try await store.apply(value)
        try await drain()
    }
    @MainActor static func order(_ store: EditorStore) -> [UInt64] { store.state["layers"].array.map { $0["id"].uint } }
    @MainActor static func barrier(_ store: EditorStore) async {
        await withCheckedContinuation { done in
            store.query(["type": "catalog"]) { _ in done.resume() }
        }
    }
    @MainActor static func run(_ platform: UInt32, theme: ColorScheme) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Initial layers did not load") { order(store).count == 2 }
        try await store.apply(["type": "set_theme", "theme": theme == .dark ? "dark" : "light"])
        for _ in 0..<22 {
            try await edit(store, ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]])
        }
        let initial = order(store), first = initial[0]
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 380, height: 260),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Layer list lifecycle check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        window.appearance = NSAppearance(named: theme == .dark ? .darkAqua : .aqua)
        let panel = JSON(["controls": [["control": "layers", "visible_in_panel": true]]])
        let host = NSHostingView(rootView: LayerPanel(store: store, panel: panel).environment(\.colorScheme, theme))
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
        let start = CGPoint(x: model.frames[first]!.grip.midX, y: model.frames[first]!.grip.midY), origin = scroll.contentView.bounds.minY
        try await send(.leftMouseDown, start, pen: true)
        try await send(.leftMouseDragged, CGPoint(x: start.x, y: viewport().maxY - 10), pen: true)
        try await wait("Grip layer edge scrolling did not retain its offscreen source") {
            scroll.contentView.bounds.minY > origin + 120 && model.contact.target?.id.hasSuffix(":layer:\(first)") == true
        }
        try require(model.contact.dragging && model.menu.isNull, "A pen grip scrolls immediately without a menu")
        let drop = CGPoint(x: start.x, y: viewport().midY)
        try await send(.leftMouseDragged, drop, pen: true)
        try await wait("The centered drag did not reach a measured row") {
            model.drag?.point == drop && model.drag?.hit != nil
        }
        await barrier(store)
        guard let hint = model.drag, let target = hint.target, let position = hint.position else {
            throw HostFailure(message: "No measured layer drop hint after scrolling")
        }
        try require(position == "above" || position == "below", "Paint layer feedback must indicate a row boundary")
        var expected = initial.filter { $0 != first }
        expected.insert(first, at: expected.firstIndex(of: target)! + (position == "below" ? 1 : 0))
        try await send(.leftMouseUp, drop, pen: true)
        try await wait("Scrolled layer drop did not match its indicated position") { order(store) == expected }
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(order(store) == initial, "One Undo must restore the offscreen row move")
        try await edit(store, ["type": "invoke", "command": "redo"])
        try require(order(store) == expected, "Redo must restore the same scrolled row move")
        note("PASS platform \(platform): native immediate pen grip drag, offscreen edge scrolling, indicated drop and Undo/Redo")

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

        let immediateInitial = order(store), immediateFirst = immediateInitial[0]
        let next = immediateInitial[1], nextFrame = model.frames[next]!.row
        let immediateStart = body(immediateFirst)
        let immediateEnd = CGPoint(x: immediateStart.x, y: nextFrame.maxY - 2)
        guard let released = model.source(at: immediateStart) else { throw HostFailure(message: "Immediate-drop source was not measured") }
        model.contact.prepare(released, device: .mouse, origin: immediateStart)
        try require(model.contact.move(to: immediateEnd), "Immediate-drop contact must start dragging")
        await barrier(store)
        guard let immediateHint = model.drag, let immediateTarget = immediateHint.target,
              let immediatePosition = immediateHint.position else { throw HostFailure(message: "Immediate-drop feedback must accept the destination") }
        var immediateOrder = immediateInitial.filter { $0 != immediateFirst }
        immediateOrder.insert(immediateFirst, at: immediateOrder.firstIndex(of: immediateTarget)! + (immediatePosition == "below" ? 1 : 0))
        model.contact.release(at: immediateEnd)
        guard let following = model.source(at: immediateStart) else { throw HostFailure(message: "The following row press was not admitted") }
        model.contact.prepare(following, device: .mouse, origin: immediateStart)
        await barrier(store); await barrier(store)
        try require(order(store) == immediateOrder && store.failure == nil,
            "A new row press before the final drop reply must preserve the completed reorder")
        model.contact.cancel()
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(order(store) == immediateInitial, "The completed drop before a new press must add exactly one Undo entry")
        note("PASS platform \(platform): completed row drop survives an immediate following press and one Undo restores order")

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
        _ = attachSurface(store, CGSize(width: 128, height: 128))
        let attached = await withCheckedContinuation { done in
            store.native!.submit(2, JSON(["type": "catalog"])) { done.resume(returning: $0 != nil) }
        }
        try require(attached && store.failure == nil, store.failure ?? "The document owner did not attach")
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { _, done in done([]) }, save: { _, _, completed in completed(nil) }, create: { _, completed in completed(newSheet([256, 256], space: "Srgb", depth: "U8")) }))
        store.invoke("new_document")
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
        let paint = order(store)[0], paper = order(store)[1]
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
        func drop(_ id: UInt64, on target: UInt64, fraction: CGFloat = 0.5, accepted: Bool = true,
                  feedbackTarget: UInt64? = nil, position: String = "into") async throws {
            let source = model.frames[id]!.name, destination = model.frames[target]!.row
            let start = CGPoint(x: source.midX, y: source.midY)
            let end = CGPoint(x: source.midX, y: destination.minY + destination.height * fraction)
            number += 1; try event(.leftMouseDown, at: start, marker: marker, number: number); try await drain(0.02)
            number += 1; try event(.leftMouseDragged, at: end, marker: marker, number: number); try await drain()
            if row(id)["locked"].bool {
                try require(!accepted && model.contact.target?.canDrag == false && !model.contact.dragging,
                    "A locked layer body must reject native pickup")
            } else {
                try await wait("Native hierarchy drag must reach its measured target") {
                    model.contact.dragging && model.drag?.hit?.target == target
                }
            }
            await barrier(store)
            try require(accepted ? model.drag?.target == (feedbackTarget ?? target) && model.drag?.position == position
                : model.drag?.target == nil && model.drag?.position == nil,
                "Shared drop feedback must accept valid targets and reject invalid targets")
            number += 1; try event(.leftMouseUp, at: end, marker: marker, number: number); try await drain()
            await barrier(store); await barrier(store)
            try require(model.contact.target == nil && model.drag == nil, "Releasing a hierarchy drop must retire native capture")
        }
        try await drop(paint, on: group)
        try await wait("Dropping on a group center must reparent the layer") { row(paint)["depth"].uint == 1 }
        try require(!row(group)["collapsed"].bool, "A successful group drop must expose its child")
        let nestedOrder = order(store), nestedRevision = store.state["revision"].uint
        try await drop(group, on: paint, accepted: false)
        try require(order(store) == nestedOrder && store.state["revision"].uint == nestedRevision && store.failure == nil,
            "An invalid descendant drop cannot modify the document or history")
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(row(paint)["depth"].uint == 0, "Undo after a rejected drop must restore the last successful reparent")

        for locked in [group, paint] {
            try await edit(store, ["type": "layer", "action": ["op": "lock", "id": locked, "value": true]])
            let before = store.state["revision"].uint, beforeOrder = order(store)
            try await drop(paint, on: group, accepted: false)
            try require(store.failure == nil && row(paint)["depth"].uint == 0 && order(store) == beforeOrder
                && store.state["revision"].uint == before, "A locked drop must preserve document and hierarchy")
            try await edit(store, ["type": "invoke", "command": "undo"])
            try require(!row(locked)["locked"].bool, "An invalid drop must not add a history entry after locking")
        }
        // The shared group interval includes 0.25 and excludes 0.75.
        try await drop(paint, on: group, fraction: 0.25)
        try await wait("The upper group boundary must reparent") { row(paint)["depth"].uint == 1 }
        try await drop(paint, on: group, fraction: 0.75, feedbackTarget: paper, position: "above")
        try await wait("The lower group boundary must insert outside the group") { row(paint)["depth"].uint == 0 }
        try require(order(store) == [group, paint, paper], "The lower group boundary must retain the indicated order above Paper")
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(row(paint)["depth"].uint == 1 && order(store) == [group, paint, paper],
            "One Undo must restore the boundary drop's group membership")
        try await edit(store, ["type": "invoke", "command": "redo"])
        try require(row(paint)["depth"].uint == 0 && order(store) == [group, paint, paper],
            "Redo must restore the indicated drop outside the group")
        try require(store.failure == nil, store.failure ?? "")
        note("PASS platform \(platform): native group reparent/boundaries, locked source/destination and descendant rejection with history retained")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { for platform: UInt32 in [0, 1] { for theme: ColorScheme in [.light, .dark] { try await run(platform, theme: theme) }; try await hierarchy(platform) }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
