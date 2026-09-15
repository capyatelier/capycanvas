import AppKit
import ImageIO

/// The real AppKit canvas, event queue, serial owner and Metal renderer. Actions
/// choose tools; pointer/key/focus events use the native window. This is component
/// evidence for both shared host configurations, not physical Pencil delivery.
@main final class CanvasNativeInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            try await drain(0.01)
        }
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-canvas-input-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let png = root.appendingPathComponent("artwork.png")
        let project = root.appendingPathComponent("rulers.capy")
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { $0(nil) }, save: { _, type, done in done(type == .capyProject ? project : png) },
            create: { _, done in done([128, 128]) }))
        let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let canvas = MacCanvasView(store: store)
        window.contentView = canvas
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { canvas.stop(); window.delegate = nil; window.contentView = nil; window.close() }
        try await wait("Metal canvas") { store.canvasSubmitted && store.snapshot["shaders_ready"].bool }
        let originalWindow = window.frame
        var deliveredEvent = -1, nextEvent = 0
        let monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp]) { event in
            if event.window === window { deliveredEvent = event.eventNumber }
            return event
        }
        defer { if let monitor { NSEvent.removeMonitor(monitor) } }
        func action(_ value: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                store.edit(value) { error in
                    if let error { done.resume(throwing: HostFailure(message: error)) }
                    else { done.resume() }
                }
            }
            try await drain(0.02)
            try require(store.failure == nil, store.failure ?? "")
        }
        func invoke(_ command: String) async throws { try await action(["type":"invoke", "command":command]) }
        func tool(_ value: Any) async throws { try await action(["type":"layer", "action":["op":"tool", "tool":value]]) }
        func enabled(_ command: String) -> Bool {
            store.state["commands"].array.first { $0["id"].string == command }?["enabled"].bool == true
        }
        func newDocument() async throws {
            try await invoke("new_document")
            if store.projectFiles.confirming { store.projectFiles.choose("discard") }
            try await wait("New 128px document") {
                !store.projectFiles.busy && !store.projectFiles.confirming && store.state["requests"].array.isEmpty
            }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            try await invoke("fit_canvas")
            try await action(["type":"set_color", "rgba":[0,0,1,1]])
        }
        func pixels() async throws -> Data {
            try? FileManager.default.removeItem(at: png)
            try await invoke("export_document")
            try await wait("Committed PNG export") {
                FileManager.default.fileExists(atPath: png.path) && !store.projectFiles.busy
            }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            guard let source = CGImageSourceCreateWithURL(png as CFURL, nil),
                let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw HostFailure(message: "No exported image") }
            try require(image.width == 128 && image.height == 128, "Export the fixture document, not its viewport")
            var bytes = Data(count: 128 * 128 * 4)
            bytes.withUnsafeMutableBytes { buffer in
                let context = CGContext(data: buffer.baseAddress, width: 128, height: 128, bitsPerComponent: 8,
                    bytesPerRow: 128 * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue)!
                context.draw(image, in: CGRect(x: 0, y: 0, width: 128, height: 128))
            }
            return bytes
        }
        func rulers() async throws -> JSON {
            try? FileManager.default.removeItem(at: project)
            try await invoke("save_document_as")
            try await wait("Committed ruler project") {
                FileManager.default.fileExists(atPath: project.path) && !store.projectFiles.busy
            }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            // Read the actual saved document's JSON manifest; ruler overlays
            // are intentionally absent from exported artwork.
            let bytes = try Data(contentsOf: project)
            try require(bytes.count >= 52 && bytes.prefix(12) == Data("CAPYRASTER\u{1}\0".utf8), "Expected indexed raster project")
            let count = bytes[12..<20].enumerated().reduce(UInt64(0)) { $0 | UInt64($1.element) << ($1.offset * 8) }
            try require(count <= bytes.count - 52, "Complete project manifest")
            return JSON(try JSONSerialization.jsonObject(with: bytes.subdata(in: 52..<(52 + Int(count)))))["document"]["rulers"]
        }
        func send(_ type: NSEvent.EventType, _ point: CGPoint, flags: NSEvent.ModifierFlags = []) async throws {
            if type == .leftMouseDown {
                try require(window.isKeyWindow, "The owned canvas must have native focus before a contact")
            }
            let camera = store.state["camera"], scale = window.backingScaleFactor
            let surface = CGPoint(x: camera["translation"][0].number + point.x * camera["zoom"].number,
                y: camera["translation"][1].number + point.y * camera["zoom"].number)
            let local = CGPoint(x: surface.x / scale, y: surface.y / scale)
            try require(canvas.bounds.contains(local), "The document point must be inside the real canvas")
            nextEvent += 1
            guard let event = NSEvent.mouseEvent(with: type, location: canvas.convert(local, to: nil),
                modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: nextEvent, clickCount: 1, pressure: 1) else { throw HostFailure(message: "No mouse event") }
            NSApp.postEvent(event, atStart: false)
            try await wait("Native event \(nextEvent) delivery") { deliveredEvent == nextEvent }
            try await drain(0.025)
        }
        func shift(_ pressed: Bool) async throws {
            try require(window.firstResponder === canvas, "The native canvas must receive modifier changes")
            guard let event = NSEvent.keyEvent(with: .flagsChanged, location: .zero,
                modifierFlags: pressed ? .shift : [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: "", charactersIgnoringModifiers: "",
                isARepeat: false, keyCode: 56) else { throw HostFailure(message: "No native modifier event") }
            NSApp.postEvent(event, atStart: false); try await drain()
        }
        func path(_ points: [CGPoint], cancel: String? = nil) async throws {
            try await send(.leftMouseDown, points[0])
            for point in points.dropFirst().dropLast() { try await send(.leftMouseDragged, point) }
            if cancel == "escape" {
                try key("\u{1b}", code: 53, window: window); try await drain()
            } else if cancel == "blur" {
                let other = NSWindow(contentRect: CGRect(x: 20, y: 20, width: 120, height: 80),
                    styleMask: [.titled], backing: .buffered, defer: false)
                other.isReleasedWhenClosed = false
                defer { other.close() }
                other.makeKeyAndOrderFront(nil)
                try await wait("Focus moved to another owned window") { other.isKeyWindow && !window.isKeyWindow }
                window.makeKeyAndOrderFront(nil)
                try await wait("Canvas focus restored") { window.isKeyWindow }
            } else if cancel == "tool" { try await invoke("pen") }
            try await send(.leftMouseUp, points.last!)
            try require(window.frame == originalWindow, "Canvas contacts must not move the OS window")
        }
        func blue(_ bytes: Data, _ x: Int, _ y: Int) -> Bool {
            let i = (y * 128 + x) * 4
            return bytes[i] < 64 && bytes[i + 1] < 64 && bytes[i + 2] > 192
        }
        let lasso = [CGPoint(x:24,y:24), CGPoint(x:52,y:24), CGPoint(x:52,y:52), CGPoint(x:104,y:52),
            CGPoint(x:104,y:76), CGPoint(x:52,y:76), CGPoint(x:52,y:104), CGPoint(x:24,y:104), CGPoint(x:24,y:24)]
        func paintLasso(_ choice: String) async throws -> Data {
            try await tool(choice); try await path(lasso)
            if choice == "select" {
                try await wait("Native freehand lasso selection") { enabled("fill_selection") }
                try await invoke("fill_selection")
            }
            let painted = try await pixels()
            try require(blue(painted,32,32) && blue(painted,80,64) && !blue(painted,80,32), "Native lasso must follow its concave path")
            return painted
        }
        for choice in ["select", "lasso_fill"] {
            try await newDocument()
            let paper = try await pixels()
            let painted = try await paintLasso(choice)
            try await invoke("undo")
            let undone = try await pixels(); try require(undone == paper, "Lasso artwork Undo restores every pixel")
            try await invoke("redo")
            let redone = try await pixels(); try require(redone == painted, "Lasso artwork Redo restores every pixel")
            try await invoke("undo")
            if choice == "select" { try await invoke("deselect") }
            for cancellation in ["escape", "blur", "tool"] {
                try await invoke("add_layer"); try await invoke("undo")
                try await tool(choice)
                try await path(lasso, cancel: cancellation)
                try require(!enabled("fill_selection") && enabled("redo"), "Cancelled \(choice) preserves selection and Redo: \(cancellation)")
                let cancelled = try await pixels()
                try require(cancelled == paper, "Cancelled \(choice) must preserve every pixel: \(cancellation)")
                let recovered = try await paintLasso(choice)
                try require(recovered == painted, "The next contact after \(cancellation) must paint the same lasso")
                try await invoke("undo")
                if choice == "select" { try await invoke("deselect") }
            }
            note("PASS platform \(platform): native \(choice) freehand path, Escape/blur/tool cancellation and exact PNG history")
        }
        let stroke = [CGPoint(x:32,y:64), CGPoint(x:48,y:88), CGPoint(x:64,y:84), CGPoint(x:80,y:80), CGPoint(x:96,y:84)]
        for kind in ["straight", "parallel", "radial"] {
            try await newDocument()
            let paper = try await pixels()
            let creation = [CGPoint(x:24,y:64), CGPoint(x:60,y:64), CGPoint(x:104,y:64)]
            let handleEdit = [CGPoint(x:kind == "radial" ? 104 : 24,y:64), CGPoint(x:30,y:48), CGPoint(x:36,y:36)]
            for cancellation in ["escape", "blur", "tool"] {
                try await invoke("add_layer"); try await invoke("undo")
                try await tool(["ruler":["kind":kind]])
                try await path(creation, cancel: cancellation)
                let cancelled = try await rulers()
                try require(cancelled.array.isEmpty && enabled("redo"), "Cancelled \(kind) creation preserves document and Redo: \(cancellation)")
            }
            try await tool(["ruler":["kind":kind]])
            try await path(creation)
            let created = try await rulers()
            try require(created.array.count == 1 && created[0]["geometry"]["kind"].string == kind, "Next contact creates exactly one \(kind) ruler")
            try await invoke("undo")
            let removed = try await rulers(); try require(removed.array.isEmpty, "Ruler creation is one Undo step")
            try await invoke("redo")
            let restored = try await rulers(); try require(restored.stableKey == created.stableKey, "Ruler creation Redo restores exact saved geometry")
            for cancellation in ["escape", "blur", "tool"] {
                try await invoke("add_layer"); try await invoke("undo")
                try await tool(["ruler":["kind":kind]])
                try await path(handleEdit, cancel: cancellation)
                let cancelled = try await rulers()
                try require(cancelled.stableKey == created.stableKey && enabled("redo"), "Cancelled \(kind) handle edit preserves geometry and Redo: \(cancellation)")
                try await tool(["ruler":["kind":kind]])
                try await path(handleEdit)
                let edited = try await rulers()
                let point = edited[0]["geometry"][kind == "radial" ? "center" : "start"]
                try require(edited.array.count == 1 && abs(point["x"].number - 36) < 0.01 && abs(point["y"].number - 36) < 0.01,
                    "Next contact after \(cancellation) must move the \(kind) handle")
                try await invoke("undo")
                let undone = try await rulers(); try require(undone.stableKey == created.stableKey, "Handle edit Undo restores exact geometry")
            }
            if kind != "radial" {
                let end = CGPoint(x:104,y:64), moved = CGPoint(x:80,y:100)
                try await send(.leftMouseDown, end); try await send(.leftMouseDragged, moved)
                try await shift(true); try await send(.leftMouseUp, moved, flags: .shift); try await shift(false)
                let constrained = try await rulers(), geometry = constrained[0]["geometry"]
                let dx = geometry["end"]["x"].number - geometry["start"]["x"].number
                let dy = geometry["end"]["y"].number - geometry["start"]["y"].number
                try require(dx > 40 && abs(dx - dy) < 0.01, "Shift must constrain the native \(kind) handle to 45 degrees")
                try await invoke("undo")
                let undone = try await rulers(); try require(undone.stableKey == created.stableKey, "Shift edit is one Undo step")
                try await invoke("redo")
                let redone = try await rulers(); try require(redone.stableKey == constrained.stableKey, "Shift edit Redo restores exact geometry")
                try await invoke("undo")
                try await send(.leftMouseDown, end); try await shift(true)
                try await send(.leftMouseDragged, moved, flags: .shift); try await shift(false)
                try await send(.leftMouseUp, moved)
                let released = try await rulers(), point = released[0]["geometry"]["end"]
                try require(abs(point["x"].number - 80) < 0.01 && abs(point["y"].number - 100) < 0.01, "Releasing Shift restores free handle movement")
                try await invoke("undo")
            }
            let untouched = try await pixels(); try require(untouched == paper, "Ruler edits and cancellations must never paint the document")
            note("PASS platform \(platform): native \(kind) ruler cancellation/recovery, handle editing, applicable Shift and exact saved geometry history")
            try await action(["type":"select_brush", "id":1])
            try await action(["type":"set_brush_size", "value":3])
            try await path(stroke)
            let constrained = try await pixels()
            let ys = (0..<128).filter { y in (20..<110).contains { x in blue(constrained,x,y) } }
            try require(!ys.isEmpty && ys.allSatisfy { abs($0 - 64) <= 4 }, "\(kind) ruler must constrain actual native mouse paint")
            try await invoke("undo")
            let undone = try await pixels(); try require(undone == paper, "Ruler-guided stroke Undo restores every pixel")
            try await invoke("redo")
            let redone = try await pixels(); try require(redone == constrained, "Ruler-guided stroke Redo restores every pixel")
            try await invoke("undo"); try await invoke("snap_rulers")
            try await path(stroke)
            let free = try await pixels()
            try require((72..<96).contains { y in (32..<104).contains { x in blue(free,x,y) } }, "Disabling \(kind) snapping must restore free painting")
            // Snap is a tool preference and survives New Document. Restore it
            // before the next ruler case starts.
            try await invoke("snap_rulers")
            note("PASS platform \(platform): native \(kind) ruler creation, constrained/free painting and exact PNG history")
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { for platform: UInt32 in [0, 1] { try await run(platform) }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
