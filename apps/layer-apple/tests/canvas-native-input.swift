import AppKit
import ImageIO
import SwiftUI

/// Public AppKit event values supplied to the real canvas callbacks. Pointer
/// contacts below still use the native queue; this does not emulate a trackpad.
private final class NavigationEvent: NSEvent {
    var point = CGPoint.zero
    var flags: NSEvent.ModifierFlags = []
    var delta = CGPoint.zero
    var precise = true
    override var locationInWindow: CGPoint { point }
    override var modifierFlags: NSEvent.ModifierFlags { flags }
    override var scrollingDeltaX: CGFloat { delta.x }
    override var scrollingDeltaY: CGFloat { delta.y }
    override var hasPreciseScrollingDeltas: Bool { precise }
    override var magnification: CGFloat { 0.25 }
    override var rotation: Float { 30 }
}

/// Standalone tablet values exercise the driver path that has no mouseDown.
private final class TabletEvent: NSEvent {
    var point = CGPoint.zero
    var force: Float = 1
    var time = ProcessInfo.processInfo.systemUptime
    var nativeType: NSEvent.EventType = .tabletPoint
    var nativeButton = 0
    override var type: NSEvent.EventType { nativeType }
    override var buttonNumber: Int { nativeButton }
    override var subtype: NSEvent.EventSubtype { .tabletPoint }
    override var locationInWindow: CGPoint { point }
    override var modifierFlags: NSEvent.ModifierFlags { [] }
    override var pressure: Float { force }
    override var timestamp: TimeInterval { time }
    override var deviceID: Int { 1 }
    override var buttonMask: NSEvent.ButtonMask { force > 0 ? .penTip : [] }
    override var tilt: NSPoint { .zero }
    override var rotation: Float { 0 }
}

/// The assembled editor, AppKit event queue, serial owner and Metal renderer.
/// Actions choose tools; pointer/key/focus events use the native window through
/// the visible editor's actual hit targets. Not physical Pencil/OS menu delivery.
@main final class CanvasNativeInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            try await drain(0.01)
        }
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        for (azimuth, x, y) in [(0.0, 1.0, 0.0), (Double.pi / 2, 0, 1), (Double.pi, -1, 0), (-Double.pi / 2, 0, -1)] {
            let tilt = StylusTilt.towardBarrel(altitude: .pi / 4, azimuth: azimuth)
            try require(abs(tilt.x - x * .pi / 4) < 1e-9 && abs(tilt.y - y * .pi / 4) < 1e-9,
                "Pencil azimuth \(azimuth) must tilt toward the barrel in view axes")
        }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-canvas-input-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let png = root.appendingPathComponent("artwork.png")
        let project = root.appendingPathComponent("rulers.capy")
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { _, done in done([]) }, save: { _, type, done in done(type == .capyProject ? project : png) },
            create: { _, done in done(JSON(["extent": [128, 128], "color": ["space": "Srgb", "depth": "U8"], "background": "White"])) }, exportOptions: { $0.choose($0.recipe) }))
        let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let host = NSHostingView(rootView: EditorView(store: store) { MacMetalCanvas(store: store) }
            .frame(minWidth: 700, minHeight: 500))
        window.contentView = host
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.delegate = nil; window.contentView = nil; window.close() }
        try await wait("Metal canvas") { store.canvasSubmitted && store.snapshot["shaders_ready"].bool }
        func findCanvas(_ view: NSView) -> MacCanvasView? {
            if let canvas = view as? MacCanvasView { return canvas }
            return view.subviews.lazy.compactMap(findCanvas).first
        }
        guard let canvas = findCanvas(host) else { throw HostFailure(message: "The editor canvas is not mounted") }
        defer { canvas.stop() }
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
            if store.projectFiles.confirming {
                func discardButton(_ view: NSView) -> NSButton? {
                    if let button = view as? NSButton, button.title == "Discard Changes" { return button }
                    return view.subviews.lazy.compactMap(discardButton).first
                }
                try await wait("Native discard confirmation") {
                    window.attachedSheet?.contentView.flatMap(discardButton)?.isEnabled == true
                }
                window.attachedSheet!.contentView.flatMap(discardButton)!.performClick(nil)
            }
            try await wait("New 128px document") {
                !store.projectFiles.busy && !store.projectFiles.confirming && store.state["requests"].array.isEmpty
            }
            try await wait("Native confirmation dismissed") { window.attachedSheet == nil && window.isKeyWindow }
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
        // Read presented pixels before mouse-up: exporting artwork would commit
        // the interaction and could conceal a stale stationary preview.
        func geometryPreview(_ name: String, constrained: Bool, at points: [CGPoint]) async throws {
            try require(points.count == 2, "Provide distinct constrained and free geometry probes")
            try await drain(0.15)
            let file = root.appendingPathComponent("preview.png")
            let capture = Process(); capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
            capture.arguments = ["-x", "-o", "-l", String(window.windowNumber), file.path]
            try capture.run(); capture.waitUntilExit()
            try require(capture.terminationStatus == 0, "Capture only the owned canvas window")
            guard let source = CGImageSourceCreateWithURL(file as CFURL, nil),
                let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
                throw HostFailure(message: "Missing presented geometry preview")
            }
            if let directory = ProcessInfo.processInfo.environment["CAPY_CANVAS_CAPTURES"] {
                let target = URL(fileURLWithPath: directory).appendingPathComponent("\(platform)-\(name).png")
                try Data(contentsOf: file).write(to: target)
            }
            let scale = CGFloat(image.width) / window.frame.width
            try require(abs(CGFloat(image.height) / scale - window.frame.height) < 1,
                "Window capture must preserve its measured aspect ratio")
            var bytes = Data(count: image.width * image.height * 4)
            bytes.withUnsafeMutableBytes { buffer in
                let context = CGContext(data: buffer.baseAddress, width: image.width, height: image.height,
                    bitsPerComponent: 8, bytesPerRow: image.width * 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue)!
                context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            }
            func outline(_ x: CGFloat, _ y: CGFloat) -> Bool {
                let camera = store.state["camera"]
                let local = CGPoint(x: (camera["translation"][0].number + x * camera["zoom"].number) / window.backingScaleFactor,
                    y: (camera["translation"][1].number + y * camera["zoom"].number) / window.backingScaleFactor)
                let screen = window.convertPoint(toScreen: canvas.convert(local, to: nil))
                let px = Int((screen.x - window.frame.minX) * scale)
                let py = Int((window.frame.maxY - screen.y) * scale)
                // Figure and ruler previews use dashed geometry.
                // Sample a small neighborhood so a dash gap cannot look absent.
                let radius = Int(3 * camera["zoom"].number / window.backingScaleFactor * scale)
                guard px - radius >= 0 && px + radius < image.width
                    && py - radius >= 0 && py + radius < image.height else { return false }
                var marked = 0
                for y in (py - radius)...(py + radius) { for x in (px - radius)...(px + radius) {
                    let i = (y * image.width + x) * 4
                    if bytes[i] < 180 && bytes[i + 1] < 180 && bytes[i + 2] < 180 { marked += 1 }
                } }
                return marked >= 3
            }
            let matches = outline(points[0].x, points[0].y) == constrained
                && outline(points[1].x, points[1].y) != constrained
            try require(matches && !outline(115, 115), "Presented preview must reflect Shift before mouse-up: \(name)")
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
            try require(bytes.count >= 52 && bytes.prefix(12) == Data("CAPYRASTER\u{7}\0".utf8), "Expected current indexed raster project")
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
            try require(host.hitTest(canvas.convert(local, to: host.superview)) === canvas,
                "Visible editor controls must not cover this drawing point")
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
        // Side buttons use AppKit's tablet-backed right/other mouse callbacks.
        // Exercise both hover-first and mid-stroke presses through the adapter.
        for button in [1, 2] {
            for heldBefore in [false, true] {
                try await newDocument()
                try await action(["type":"set_brush_size", "value":4])
                try await tool(["figure":["shape":"line", "paint":"outline"]])
                let paper = try await pixels(), camera = store.state["camera"]
                let tablet = TabletEvent()
                func pose(_ x: Double, force: Float) {
                    let scale = window.backingScaleFactor
                    tablet.point = canvas.convert(CGPoint(
                        x: (camera["translation"][0].number + x * camera["zoom"].number) / scale,
                        y: (camera["translation"][1].number + 64 * camera["zoom"].number) / scale), to: nil)
                    tablet.force = force; tablet.time = ProcessInfo.processInfo.systemUptime
                }
                func side(_ down: Bool) async throws {
                    tablet.nativeButton = button
                    tablet.nativeType = button == 1 ? (down ? .rightMouseDown : .rightMouseUp)
                        : (down ? .otherMouseDown : .otherMouseUp)
                    if button == 1 {
                        if down { canvas.rightMouseDown(with: tablet) } else { canvas.rightMouseUp(with: tablet) }
                    } else {
                        if down { canvas.otherMouseDown(with: tablet) } else { canvas.otherMouseUp(with: tablet) }
                    }
                    try await drain(0.025)
                }
                pose(24, force: 0)
                if heldBefore { try await side(true) }
                pose(24, force: 1); tablet.nativeButton = 0; tablet.nativeType = .leftMouseDown
                canvas.mouseDown(with: tablet); try await drain(0.025)
                if !heldBefore { try await side(true) }
                pose(72, force: 1); tablet.nativeButton = 0; tablet.nativeType = .leftMouseDragged
                canvas.mouseDragged(with: tablet); try await drain(0.025)
                if !heldBefore { try await side(false) }
                pose(96, force: 0); tablet.nativeButton = 0; tablet.nativeType = .leftMouseUp
                canvas.mouseUp(with: tablet); try await drain(0.025)
                if heldBefore { try await side(false) }
                let painted = try await pixels()
                try require(blue(painted,32,64) && blue(painted,88,64), "Side buttons preserve the complete pen line")
                try require(store.state["camera"]["translation"].array.map(\.number)
                    == camera["translation"].array.map(\.number), "Pen buttons cannot pan the canvas")
                try await invoke("undo")
                let undone = try await pixels(); try require(undone == paper, "One Undo removes the pen line")
                try await invoke("redo")
                let redone = try await pixels(); try require(redone == painted, "Redo restores the pen line")
            }
        }
        for button in [1, 2] {
            try await newDocument()
            try await action(["type":"set_brush_size", "value":4])
            try await tool(["figure":["shape":"line", "paint":"outline"]])
            let paper = try await pixels(), camera = store.state["camera"]
            let tablet = TabletEvent()
            tablet.nativeButton = button
            func chord(_ x: Double, force: Float, _ type: NSEvent.EventType) async throws {
                let scale = window.backingScaleFactor
                tablet.point = canvas.convert(CGPoint(
                    x: (camera["translation"][0].number + x * camera["zoom"].number) / scale,
                    y: (camera["translation"][1].number + 64 * camera["zoom"].number) / scale), to: nil)
                tablet.force = force; tablet.time = ProcessInfo.processInfo.systemUptime; tablet.nativeType = type
                switch type {
                case .rightMouseDown: canvas.rightMouseDown(with: tablet)
                case .rightMouseDragged: canvas.rightMouseDragged(with: tablet)
                case .rightMouseUp: canvas.rightMouseUp(with: tablet)
                case .otherMouseDown: canvas.otherMouseDown(with: tablet)
                case .otherMouseDragged: canvas.otherMouseDragged(with: tablet)
                default: canvas.otherMouseUp(with: tablet)
                }
                try await drain(0.025)
            }
            let (down, dragged, up): (NSEvent.EventType, NSEvent.EventType, NSEvent.EventType) = button == 1
                ? (.rightMouseDown, .rightMouseDragged, .rightMouseUp) : (.otherMouseDown, .otherMouseDragged, .otherMouseUp)
            try await chord(24, force: 0, down)
            try await chord(24, force: 1, dragged)
            try await chord(72, force: 1, dragged)
            try await chord(96, force: 0, dragged)
            try await chord(96, force: 0, up)
            let painted = try await pixels()
            try require(blue(painted,32,64) && blue(painted,64,64), "A tip chorded with a held side button still draws")
            try require(store.state["camera"]["translation"].array.map(\.number)
                == camera["translation"].array.map(\.number), "A held side button cannot pan the canvas")
            try await invoke("undo")
            let undone = try await pixels(); try require(undone == paper, "One Undo removes the chorded line")
        }
        func moveOverCanvas() async throws {
            let point = canvas.convert(CGPoint(x: canvas.bounds.midX, y: canvas.bounds.midY), to: nil)
            guard let event = NSEvent.mouseEvent(with: .mouseMoved, location: point, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                eventNumber: 0, clickCount: 0, pressure: 0) else { throw HostFailure(message: "No mouse event") }
            canvas.mouseMoved(with: event); try await drain(0.05)
        }
        try await moveOverCanvas()
        try require(NSCursor.current.image.size == NSSize(width: 1, height: 1), "The system arrow hides over the canvas")
        let painting = store.state["commands"].array.first {
            $0["selected"].bool && ["pen", "pencil", "brush", "drawing_brush", "airbrush"].contains($0["id"].string)
        }?["id"].string
        try await invoke("hand")
        try await moveOverCanvas()
        try require(NSCursor.current == NSCursor.openHand, "The Hand tool shows the open hand over the canvas")
        try await invoke(painting ?? "pen")
        try await moveOverCanvas()
        try require(NSCursor.current.image.size == NSSize(width: 1, height: 1), "Leaving the Hand tool hides the cursor again")
        // Match the existing UIKit figure checks with actual AppKit mouse and
        // modifier delivery through the assembled editor's canvas hit target.
        // Changing Shift while stationary must affect the committed figure;
        // the following unmodified contact must not inherit the previous chord.
        for shape in ["line", "rectangle", "ellipse"] {
            try await newDocument()
            try await action(["type":"set_brush_size", "value":4])
            try await tool(["figure":["shape":shape, "paint":shape == "line" ? "outline" : "fill"]])
            let paper = try await pixels()
            let previewPoints = shape == "line" ? [CGPoint(x:50,y:50), CGPoint(x:60,y:46)]
                : [CGPoint(x:60,y:96), CGPoint(x:60,y:68)]
            var constrainedPixels: Data?
            var freePixels: Data?
            for (name, initial, final) in [
                ("held before contact", true, true),
                ("pressed after movement", false, true),
                ("released after movement", true, false),
                ("free next contact", false, false)
            ] {
                try await send(.leftMouseDown, CGPoint(x:24,y:24), flags: initial ? .shift : [])
                try await send(.leftMouseDragged, CGPoint(x:96,y:68), flags: initial ? .shift : [])
                try await geometryPreview("\(shape)-\(name)-before", constrained: initial, at: previewPoints)
                if initial != final {
                    try await shift(final)
                    try await geometryPreview("\(shape)-\(name)-after", constrained: final, at: previewPoints)
                }
                try await send(.leftMouseUp, CGPoint(x:96,y:68), flags: final ? .shift : [])
                try await shift(false)
                let painted = try await pixels()
                if shape == "line" {
                    try require(blue(painted,50,50) == final && blue(painted,60,46) != final,
                        "Shift must constrain the native line to 45 degrees: \(name)")
                } else {
                    try require(blue(painted,60,46) && blue(painted,60,84) == final,
                        "Shift must make the native \(shape) square or circular: \(name)")
                }
                try require(!blue(painted,115,115), "A figure must preserve outside artwork")
                if final {
                    if let constrainedPixels { try require(painted == constrainedPixels, "Stationary Shift press must match a held chord") }
                    constrainedPixels = painted
                } else {
                    if let freePixels { try require(painted == freePixels, "The next contact must not retain stale Shift") }
                    freePixels = painted
                }
                try await invoke("undo")
                let undone = try await pixels(); try require(undone == paper, "Figure Undo restores every pixel")
                try await invoke("redo")
                let redone = try await pixels(); try require(redone == painted, "Figure Redo restores every pixel")
                try await invoke("undo")
                note("PASS platform \(platform): native \(shape), Shift \(name), presented preview, constraint geometry and exact PNG history")
            }
        }
        // Verify the native navigation adapter against camera geometry and
        // exported artwork, including contact suppression and resumption.
        do {
            try await newDocument()
            let paper = try await pixels(), painted = try await paintLasso("select")
            let event = NavigationEvent(), scale = window.backingScaleFactor
            let initial = store.state["camera"]
            let anchor = CGPoint(x: initial["translation"][0].number + 64 * initial["zoom"].number,
                y: initial["translation"][1].number + 64 * initial["zoom"].number)
            let local = CGPoint(x: anchor.x / scale, y: anchor.y / scale)
            try require(host.hitTest(canvas.convert(local, to: host.superview)) === canvas,
                "Navigation must target the visible canvas")
            event.point = canvas.convert(local, to: nil)
            func flush() async throws {
                await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
                    store.native!.submit(2, JSON(["type":"catalog"])) { _ in
                        DispatchQueue.main.async { done.resume() }
                    }
                }
                try require(store.failure == nil, store.failure ?? "")
            }
            func near(_ a: Double, _ b: Double) -> Bool { abs(a - b) < 0.005 }
            func sameCamera(_ before: JSON) -> Bool {
                ["translation", "zoom", "rotation"].allSatisfy {
                    store.state["camera"][$0].stableKey == before[$0].stableKey
                }
            }
            for precise in [true, false] {
                let before = store.state["camera"], unit = precise ? 1.0 : 16.0
                event.precise = precise; event.delta = CGPoint(x: 3, y: 4)
                canvas.scrollWheel(with: event); try await flush()
                let after = store.state["camera"]
                try require(near(after["translation"][0].number - before["translation"][0].number, 3 * unit * scale)
                    && near(after["translation"][1].number - before["translation"][1].number, 4 * unit * scale),
                    "Wheel and precise scrolling must preserve native direction and backing density")
            }
            var before = store.state["camera"]
            event.precise = true; event.flags = .shift; event.delta = CGPoint(x: 2, y: 7)
            canvas.scrollWheel(with: event); try await flush()
            try require(near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 9 * scale)
                && near(store.state["camera"]["translation"][1].number, before["translation"][1].number),
                "Shift-scroll must pan horizontally")
            before = store.state["camera"]
            event.flags = .control; event.delta = CGPoint(x: 0, y: 40)
            canvas.scrollWheel(with: event); try await flush()
            try require(store.state["camera"]["zoom"].number > before["zoom"].number
                && near(store.state["camera"]["rotation"].number, before["rotation"].number),
                "Control-scroll must zoom without rotating")
            before = store.state["camera"]
            event.flags = []; canvas.magnify(with: event); try await flush()
            try require(near(store.state["camera"]["zoom"].number, before["zoom"].number * 1.25),
                "Pinch must apply incremental magnification")
            for (i, coordinate) in [anchor.x, anchor.y].enumerated() {
                try require(near(store.state["camera"]["translation"][i].number,
                    coordinate + (before["translation"][i].number - coordinate) * 1.25),
                    "Pinch must retain the document point under its physical anchor")
            }
            before = store.state["camera"]
            canvas.rotate(with: event); try await flush()
            let angle = -Double.pi / 6
            let dx = before["translation"][0].number - anchor.x, dy = before["translation"][1].number - anchor.y
            try require(near(store.state["camera"]["rotation"].number - before["rotation"].number, angle)
                && near(store.state["camera"]["translation"][0].number, anchor.x + cos(angle) * dx - sin(angle) * dy)
                && near(store.state["camera"]["translation"][1].number, anchor.y + sin(angle) * dx + cos(angle) * dy),
                "Rotation must preserve its anchor and native direction")
            try await invoke("fit_canvas")
            before = store.state["camera"]
            try await send(.leftMouseDown, lasso[0]); try await send(.leftMouseDragged, lasso[1])
            canvas.scrollWheel(with: event); canvas.magnify(with: event); canvas.rotate(with: event)
            try await flush()
            try require(sameCamera(before), "Navigation cannot move the camera during a captured contact")
            store.input(["type":"blur"]); try await flush()
            // An owner-query acknowledgement does not finish queued input.
            // Complete a real renderer frame before testing idle navigation.
            let now = FrameTrace.now()
            await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
                store.native!.frame(now: now, target: now + 16_000_000) { _, _, _ in
                    DispatchQueue.main.async { done.resume() }
                }
            }
            try await flush()
            // No old mouse-up: the existing interruption callback must also
            // release wheel/trackpad admission, not just the next stroke.
            canvas.scrollWheel(with: event); try await flush()
            try require(!sameCamera(before), "Fresh navigation must work after an interrupted contact")
            let navigated = try await pixels()
            try require(navigated == painted, "Navigation and cancelled contact must preserve every artwork pixel")
            try await invoke("undo")
            let undone = try await pixels(); try require(undone == paper, "Navigation cannot add an artwork Undo step")
            try await invoke("redo")
            let redone = try await pixels(); try require(redone == painted, "Navigation must retain exact artwork Redo")
            try require(window.frame == originalWindow, "Navigation cannot move or resize the native window")
            note("PASS platform \(platform): wheel/precise scrolling, Shift/Control, anchored pinch/rotation, contact exclusion/recovery and exact PNG history")
        }
        for interruption in ["suspend", "restart"] {
            try await newDocument()
            let paper = try await pixels()
            let painted = try await paintLasso("select")
            try await invoke("undo"); try await invoke("deselect")
            try await invoke("add_layer"); try await invoke("undo")
            try require(!enabled("fill_selection") && enabled("redo"), "Start with no selection and an existing Redo entry")
            try await send(.leftMouseDown, lasso[0])
            try await send(.leftMouseDragged, lasso[1])
            if interruption == "suspend" {
                // Use the application's sleep/wake callbacks without sleeping
                // the machine or changing its real windows.
                EditorStore.suspendWorkspaces(); EditorStore.resumeWorkspaces()
            } else {
                store.restartCanvas()
                try await wait("Restarted Metal canvas") {
                    !store.restartingCanvas && store.canvasSubmitted && store.snapshot["shaders_ready"].bool
                }
            }
            try await drain()
            // An interrupted device may never deliver its old mouseUp. Its
            // remaining movement must not resume a cancelled path, and the
            // next press must start a fresh contact.
            try await send(.leftMouseDragged, lasso[2])
            let cancelled = try await pixels()
            try require(cancelled == paper && !enabled("fill_selection") && enabled("redo"),
                "\(interruption) must cancel the unfinished lasso and retain artwork history: pixels=\(cancelled == paper), selection=\(enabled("fill_selection")), redo=\(enabled("redo"))")
            let recovered = try await paintLasso("select")
            try require(recovered == painted, "A fresh contact after \(interruption) must work without the old mouseUp")
            try await invoke("undo")
            let undone = try await pixels()
            try require(undone == paper, "The first stroke after \(interruption) remains one Undo step")
            note("PASS platform \(platform): \(interruption) without mouseUp, stale movement, fresh contact and exact PNG history")
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
                let previewPoints = [CGPoint(x:50,y:90), CGPoint(x:60,y:87)]
                try await send(.leftMouseDown, end); try await send(.leftMouseDragged, moved)
                try await geometryPreview("\(kind)-press-before", constrained: false, at: previewPoints)
                try await shift(true)
                try await geometryPreview("\(kind)-press-after", constrained: true, at: previewPoints)
                try await send(.leftMouseUp, moved, flags: .shift); try await shift(false)
                let constrained = try await rulers(), geometry = constrained[0]["geometry"]
                let dx = geometry["end"]["x"].number - geometry["start"]["x"].number
                let dy = geometry["end"]["y"].number - geometry["start"]["y"].number
                try require(dx > 40 && abs(dx - dy) < 0.01, "Shift must constrain the native \(kind) handle to 45 degrees")
                try await invoke("undo")
                let undone = try await rulers(); try require(undone.stableKey == created.stableKey, "Shift edit is one Undo step")
                try await invoke("redo")
                let redone = try await rulers(); try require(redone.stableKey == constrained.stableKey, "Shift edit Redo restores exact geometry")
                try await invoke("undo")
                // A control can forward a captured chord without updating the
                // canvas adapter's modifier cache. The next contact is decisive.
                store.input(["type":"key", "key":"F1", "pressed":false,
                    "modifiers":["command":false, "alt":false, "shift":true]])
                try await path([end, moved])
                let freeAfterControl = try await rulers(), freePoint = freeAfterControl[0]["geometry"]["end"]
                try require(abs(freePoint["x"].number - 80) < 0.01 && abs(freePoint["y"].number - 100) < 0.01,
                    "A new Mac contact must clear stale Shift from another control")
                try await invoke("undo")
                store.input(["type":"key", "key":"F1", "pressed":false,
                    "modifiers":["command":false, "alt":false, "shift":true]])
                let tablet = TabletEvent(), camera = store.state["camera"], scale = window.backingScaleFactor
                for (point, force): (CGPoint, Float) in [(end,1),(moved,1),(moved,0)] {
                    let local = CGPoint(x:(camera["translation"][0].number + point.x * camera["zoom"].number) / scale,
                        y:(camera["translation"][1].number + point.y * camera["zoom"].number) / scale)
                    try require(host.hitTest(canvas.convert(local, to: host.superview)) === canvas,
                        "Standalone tablet samples target the visible canvas")
                    tablet.point = canvas.convert(local, to:nil); tablet.force = force
                    tablet.time = ProcessInfo.processInfo.systemUptime
                    canvas.tabletPoint(with:tablet); try await drain(0.025)
                }
                let freeAfterTablet = try await rulers(), tabletPoint = freeAfterTablet[0]["geometry"]["end"]
                try require(abs(tabletPoint["x"].number - 80) < 0.01 && abs(tabletPoint["y"].number - 100) < 0.01,
                    "A standalone tablet contact must clear stale Shift from another control")
                try await invoke("undo")
                try await send(.leftMouseDown, end); try await shift(true)
                try await send(.leftMouseDragged, moved, flags: .shift)
                try await geometryPreview("\(kind)-release-before", constrained: true, at: previewPoints)
                try await shift(false)
                try await geometryPreview("\(kind)-release-after", constrained: false, at: previewPoints)
                try await send(.leftMouseUp, moved)
                let released = try await rulers(), point = released[0]["geometry"]["end"]
                try require(abs(point["x"].number - 80) < 0.01 && abs(point["y"].number - 100) < 0.01, "Releasing Shift restores free handle movement")
                try await invoke("undo")
                note("PASS platform \(platform): native \(kind) stationary Shift press/release previews before mouse-up")
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
