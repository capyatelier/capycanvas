import UIKit
import ImageIO

// Supplied UIKit contacts through the production canvas and Rust owner. Saved
// ruler geometry checks the modifier's effect, without keyboard/UI automation.
private final class Contact: UITouch {
    var point = CGPoint(x: 500, y: 400)
    var time = ProcessInfo.processInfo.systemUptime
    var device: UITouch.TouchType = .indirectPointer
    var pressure: CGFloat = 1
    var altitude: CGFloat = .pi / 2
    override var type: UITouch.TouchType { device }
    override var timestamp: TimeInterval { time }
    override func preciseLocation(in view: UIView?) -> CGPoint { point }
    override var estimationUpdateIndex: NSNumber? { nil }
    override var estimatedPropertiesExpectingUpdates: UITouch.Properties { [] }
    override var force: CGFloat { pressure }
    override var maximumPossibleForce: CGFloat { 1 }
    override var altitudeAngle: CGFloat { altitude }
    override func azimuthAngle(in view: UIView?) -> CGFloat { 0 }
    override var rollAngle: CGFloat { 0 }
}
private final class ContactEvent: UIEvent {
    var flags: UIKeyModifierFlags = []
    var buttons: UIEvent.ButtonMask = .primary
    var nativeType: UIEvent.EventType = .touches
    override var type: UIEvent.EventType { nativeType }
    override var modifierFlags: UIKeyModifierFlags { flags }
    override var buttonMask: UIEvent.ButtonMask { buttons }
    override func coalescedTouches(for touch: UITouch) -> [UITouch]? { nil }
    override func predictedTouches(for touch: UITouch) -> [UITouch]? { nil }
}
private final class Scroll: UIPanGestureRecognizer {
    var phase: UIGestureRecognizer.State = .changed
    var delta = CGPoint.zero
    var flags: UIKeyModifierFlags = []
    override var state: UIGestureRecognizer.State { get { phase } set { phase = newValue } }
    override var modifierFlags: UIKeyModifierFlags { flags }
    override func location(in view: UIView?) -> CGPoint { CGPoint(x: 500, y: 400) }
    override func translation(in view: UIView?) -> CGPoint { delta }
    override func setTranslation(_ translation: CGPoint, in view: UIView?) { delta = translation }
}

private final class BubbleHover: UIHoverGestureRecognizer {
    var phase: UIGestureRecognizer.State = .began
    var point = CGPoint.zero
    override var state: UIGestureRecognizer.State { get { phase } set { phase = newValue } }
    override func location(in view: UIView?) -> CGPoint { point }
}

private final class NativePinch: UIPinchGestureRecognizer {
    var phase: UIGestureRecognizer.State = .possible
    override var state: UIGestureRecognizer.State { get { phase } set { phase = newValue } }
    override func location(in view: UIView?) -> CGPoint { CGPoint(x: 500, y: 400) }
}
private final class NativeRotation: UIRotationGestureRecognizer {
    var phase: UIGestureRecognizer.State = .possible
    override var state: UIGestureRecognizer.State { get { phase } set { phase = newValue } }
    override func location(in view: UIView?) -> CGPoint { CGPoint(x: 500, y: 400) }
}

@MainActor private final class ModifierChecks: NSObject, UIApplicationDelegate, UIWindowSceneDelegate {
    private var completedGroups = 0
    private let runID = ProcessInfo.processInfo.environment["CAPY_INPUT_RUN"] ?? UUID().uuidString
    private var vectorsOnly: Bool { ProcessInfo.processInfo.environment["CAPY_VECTOR_INPUT_ONLY"] == "1" }
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Input Checks", sessionRole: session.role)
        configuration.delegateClass = ModifierChecks.self
        return configuration
    }
    func application(_ application: UIApplication,
        didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        Task { @MainActor in
            do {
                try await run(); try report(nil)
                print("PASS: UIKit contact input, artwork and history"); exit(0)
            } catch {
                try? report(error.localizedDescription)
                print("FAIL: \(error.localizedDescription)"); exit(1)
            }
        }
        return true
    }
    private func passed(_ message: String) {
        completedGroups += 1; FileHandle.standardError.write(Data((message + "\n").utf8))
    }
    private func report(_ failure: String?) throws {
        // Device console delivery is optional; bind the durable result to this
        // launch so an earlier successful run cannot satisfy a later check.
        let folder = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let data = try JSONSerialization.data(withJSONObject: ["run_id":runID,
            "passed":failure == nil, "groups":completedGroups, "failure":failure as Any? ?? NSNull()])
        try data.write(to: folder.appendingPathComponent("canvas-input-result.json"), options:.atomic)
    }
    private func require(_ value: Bool, _ message: String) throws {
        guard value else { throw HostFailure(message: message) }
    }
    private func run() async throws {
        try require(StorageLocations.installation != nil, "Prepare private process temporary storage")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-modifiers-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("ruler.capy")
        let store = EditorStore(platform: 0,
            persistence: EditorPersistence(root: root.appendingPathComponent("state")), managedWorkspaces: false)
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { _, done in done([]) }, save: { _, _, done in done(nil) },
            create: { _, done in done(nil) }, export: { staging, done in
                do { try FileManager.default.copyItem(at: staging, to: url); done(url) }
                catch { store.failure = error.localizedDescription; done(nil) }
            }, exportOptions: { $0.choose($0.recipe) }))
        let canvas = CanvasView(store: store)
        let renderLayer = canvas.layer.sublayers?.compactMap { $0 as? ObservedMetalLayer }.first
        try require(!(canvas.layer is CAMetalLayer) && renderLayer != nil && renderLayer?.delegate == nil,
            "The render owner must configure a standalone Metal layer without a UIView delegate")
        canvas.contentScaleFactor = 1
        defer { canvas.stop() }
        let native = store.native!
        let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 1200, height: 900)
        native.attach(surface, width: 1200, height: 900, scale: 1)
        defer { native.detach(); withExtendedLifetime(surface) {} }
        let deadline = Date().addingTimeInterval(30)
        while !store.snapshot["shaders_ready"].bool {
            try require(Date() < deadline && store.failure == nil, store.failure ?? "Canvas startup")
            let now = FrameTrace.now()
            await withCheckedContinuation { done in
                native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() }
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        func flush() async throws {
            await withCheckedContinuation { done in
                native.submit(2, JSON(["type":"catalog"])) { _ in DispatchQueue.main.async { done.resume() } }
            }
            try require(store.failure == nil, store.failure ?? "")
        }
        func action(_ value: [String: Any]) async throws {
            store.dispatch(value); try await flush()
        }
        do {
            let input = ReorderInputView(frame: CGRect(x: 0, y: 0, width: 1200, height: 900))
            input.model = store.workspace.input
            let window = UIWindow(frame: input.frame)
            window.addSubview(input); input.validate()
            defer { input.detach(); input.removeFromSuperview(); withExtendedLifetime(window) {} }
            let types = [UITouch.TouchType.pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
            try require(window.gestureRecognizers?.contains { $0.name == "bubble-hover" && $0.allowedTouchTypes == types } == true,
                "Bubble hover accepts both Pencil and trackpad pointer records")
            let grab: [String: Any] = ["kind": "variant", "variant": ["type": "brush_preset", "id": 1]]
            for phase in ["down", "up"] {
                try await action(["type": "bubble", "action": ["type": "drag", "grab": grab,
                    "phase": phase, "position": [500, 400], "viewport": [1200, 900]]])
            }
            guard let bubble = store.bubbles.items.first else { throw HostFailure(message: "Hover needs its real shared brush bubble") }
            try await wait("Positioned native hover bubble", seconds: 5, failure: { store.failure }, step: {
                await frame(native); store.bubbles.tick(CACurrentMediaTime()); try await flush()
            }) { bubble.placement.positioned }
            let hover = BubbleHover(); hover.point = bubble.placement.center
            input.hoveredBubbles(hover)
            try require(!bubble.hover.isNull, "Native hover highlights the shared bubble hit")
            hover.phase = .changed; hover.point.x = -10; input.hoveredBubbles(hover)
            try require(bubble.hover.isNull, "Leaving the editor clears bubble hover")
            hover.point = bubble.placement.center; input.hoveredBubbles(hover)
            try require(!bubble.hover.isNull, "Returning hover restores the highlight")
            hover.phase = .ended; input.hoveredBubbles(hover)
            try require(bubble.hover.isNull, "Native hover end clears the highlight")
            try await action(["type": "bubble", "action": ["type": "put_back_all"]])
            passed("PASS: UIKit bubble hover highlights and clears with Pencil and trackpad pointer records")
        }
        func rulers() async throws -> JSON {
            let prepared = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try require(prepared, "Prepare completed ruler input")
            try await flush()
            try? FileManager.default.removeItem(at: url)
            try await action(["type": "invoke", "command": "save_document_as"])
            let deadline = Date().addingTimeInterval(30)
            while !FileManager.default.fileExists(atPath: url.path) || store.projectFiles.busy {
                try require(Date() < deadline && store.projectFiles.error == nil, store.projectFiles.error ?? "Save ruler package")
                try await Task.sleep(for: .milliseconds(10))
            }
            return try savedRulers(url)
        }
        try await flush()
        store.cameraRevision = store.state["camera"]["revision"].uint
        if !vectorsOnly {
        for device: UITouch.TouchType in [.indirectPointer, .pencil] {
            for (name, flags, constrained) in [
                ("held before contact", [UIKeyModifierFlags.shift, .shift, .shift], true),
                ("released during movement", [.shift, [], []], false),
                ("pressed during movement", [[], .shift, .shift], true),
                ("released on up", [.shift, .shift, []], false),
                ("pressed on up", [[], [], .shift], true),
                ("free next contact", [[], [], []], false),
                ("released outside canvas", [[], [], []], false),
                ("held after interruption", [.shift, .shift, .shift], true)
            ] {
                try await action(["type":"layer", "action":["op":"tool", "tool":["ruler":["kind":"straight"]]]])
                let touch = Contact(), event = ContactEvent(); touch.device = device
                if name == "released outside canvas" {
                    // Editor controls can forward a captured chord directly to
                    // shared input while the canvas does not receive key events.
                    store.input(["type":"key", "key":"F1", "pressed":false,
                        "modifiers":["command":false, "alt":false, "shift":true]])
                }
                if name == "held after interruption" {
                    event.flags = .shift
                    canvas.touchesBegan([touch], with: event)
                    touch.point = CGPoint(x: 650, y: 490); touch.time += 0.01
                    canvas.touchesMoved([touch], with: event)
                    store.input(["type":"blur"])
                    try require(try await rulers().array.isEmpty, "Interrupted ruler must not commit")
                    // A new began phase is a new contact even if UIKit reuses
                    // a touch identity whose previous terminal event was lost.
                    touch.point = CGPoint(x: 500, y: 400); touch.time += 0.01
                }
                event.flags = flags[0]; canvas.touchesBegan([touch], with: event)
                try require(canvas.contacts.count == 1, "\(name): a new began phase must capture the contact")
                touch.point = CGPoint(x: 650, y: 490); touch.time += 0.01
                event.flags = flags[1]; canvas.touchesMoved([touch], with: event)
                touch.time += 0.01
                event.flags = flags[2]; canvas.touchesEnded([touch], with: event)
                try await flush()
                let saved = try await rulers(), geometry = saved[0]["geometry"]
                try require(saved.array.count == 1, "Exactly one ruler")
                let dx = geometry["end"]["x"].number - geometry["start"]["x"].number
                let dy = geometry["end"]["y"].number - geometry["start"]["y"].number
                try require(dx > 0 && dy > 0 && (constrained ? abs(dx - dy) < 0.01 : abs(dy / dx - 0.6) < 0.001),
                    "\(device) \(name): expected \(constrained ? "45-degree constraint" : "free movement"), got \(dx),\(dy)")
                try await action(["type":"invoke", "command":"undo"])
                try require(try await rulers().array.isEmpty, "Ruler must be one Undo step")
                try await action(["type":"invoke", "command":"redo"])
                try require(try await rulers().stableKey == saved.stableKey, "Redo restores exact ruler geometry")
                try await action(["type":"invoke", "command":"undo"])
                passed("PASS: \(device) \(name), saved geometry and one-step Undo/Redo")
            }
        }
        for fingerFirst in [false, true] {
            let pen = Contact(), finger = Contact(), event = ContactEvent()
            pen.device = .pencil; finger.device = .direct
            if fingerFirst {
                canvas.touchesBegan([finger], with: event)
                canvas.touchesBegan([pen], with: event)
            } else {
                canvas.touchesBegan([pen], with: event)
                canvas.touchesBegan([finger], with: event)
            }
            try require(canvas.contacts.count == 1 && canvas.contacts[ObjectIdentifier(pen)] != nil
                && canvas.ignoredContacts.contains(ObjectIdentifier(finger)), "Pencil must exclude fingers in either arrival order")
            finger.time += 0.01; canvas.touchesCancelled([finger], with: event)
            try require(!canvas.ignoredContacts.contains(ObjectIdentifier(finger)), "Terminal ignored contact must retire")
            finger.time += 0.01; canvas.touchesBegan([finger], with: event)
            try require(canvas.contacts.count == 1 && canvas.ignoredContacts.contains(ObjectIdentifier(finger)),
                "A reused finger identity must still obey active-Pencil palm rejection")
            pen.time += 0.01; canvas.touchesCancelled([pen], with: event)
            finger.time += 0.01; canvas.touchesEnded([finger], with: event)
            try require(canvas.contacts.isEmpty && canvas.ignoredContacts.isEmpty, "All cancelled/ignored contacts must retire")
            try require(try await rulers().array.isEmpty, "Palm exclusion and cancellation must not commit a ruler")
            passed("PASS: palm exclusion, \(fingerFirst ? "finger" : "Pencil") first, identity reuse and cancellation")
        }
        }
        try await artwork(store: store, canvas: canvas, root: root)
    }

    private func artwork(store: EditorStore, canvas: CanvasView, root: URL) async throws {
        let native = store.native!, png = root.appendingPathComponent("artwork.png")
        let vectorPackage = root.appendingPathComponent("vectors.capy")
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { _, done in done([]) }, save: { _, _, done in done(nil) },
            create: { _, done in done([["op": "unit", "unit": "px"], ["op": "width", "value": 128], ["op": "height", "value": 128],
                ["op": "paper"], ["op": "paper_relief", "value": 0], ["op": "use_paper"], ["op": "paper_color"],
                ["op": "paper_color_value", "color": ["space": "Srgb", "rgba": [1, 1, 1, 1]]], ["op": "use_paper_color"]]) },
            export: { staging, done in
                let destination = staging.pathExtension == "capy" ? vectorPackage : png
                do { try FileManager.default.copyItem(at: staging, to: destination); done(destination) }
                catch { store.failure = error.localizedDescription; done(nil) }
            }, exportOptions: { $0.choose($0.recipe) }))
        func flush() async throws {
            // Live transform previews are intentionally not recoverable yet.
            // Drive the ordinary renderer frame rather than requesting a save.
            let now = FrameTrace.now()
            await withCheckedContinuation { done in
                native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() }
            }
            await withCheckedContinuation { done in
                native.submit(2, JSON(["type":"catalog"])) { _ in DispatchQueue.main.async { done.resume() } }
            }
            try require(store.failure == nil, store.failure ?? "Prepare canvas work")
        }
        func wait(_ description: String, _ ready: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(30)
            while !ready() {
                if Date() >= deadline {
                    let context = JSON(["drawing":store.drawingIdentity,
                        "file":store.state["document_file"].raw, "layers":store.state["layers"].raw,
                        "tool":store.state["layer_tools"].raw, "requests":store.state["requests"].raw,
                        "canvas_ready":store.snapshot["canvas_ready"].raw, "busy":store.projectFiles.busy,
                        "package":store.projectFiles.packageSummary?.raw ?? NSNull(),
                        "profile":store.projectFiles.pendingProfile?.raw ?? NSNull(),
                        "project_error":store.projectFiles.error as Any? ?? NSNull()])
                    throw HostFailure(message: description + ": " + context.stableKey)
                }
                try await flush(); try await Task.sleep(for: .milliseconds(10))
            }
        }
        func action(_ value: [String: Any]) async throws {
            store.dispatch(value)
            await withCheckedContinuation { done in native.submit(2, JSON(["type":"catalog"])) { _ in done.resume() } }
            try await flush()
        }
        func invoke(_ command: String) async throws { try await action(["type":"invoke", "command":command]) }
        func tool(_ value: Any) async throws {
            try await action(["type":"layer", "action":["op":"tool", "tool":value]])
        }
        func newDocument() async throws {
            let identity = store.drawingIdentity
            try await invoke("new_document")
            try await wait("Create replacement drawing") { store.drawingIdentity != identity && !store.projectFiles.busy }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            try await invoke("fit_canvas")
            try await wait("Prepare replacement drawing") { store.snapshot["brush_ready"].bool }
        }
        func path(_ points: [CGPoint], device: UITouch.TouchType, modifiers: UIKeyModifierFlags = [], cancel: Bool = false,
            pressures: [CGFloat] = [], altitude: CGFloat = .pi / 2) async throws {
            try require(pressures.isEmpty || pressures.count == points.count, "Match supplied pressure samples to contact positions")
            let touch = Contact(), event = ContactEvent(); touch.device = device
            touch.pressure = pressures.first ?? 1; touch.altitude = altitude
            event.flags = modifiers
            let camera = store.state["camera"]
            func position(_ p: CGPoint) throws -> CGPoint {
                let angle = CGFloat(camera["rotation"].number), zoom = CGFloat(camera["zoom"].number)
                let x = p.x * (camera["flipped"][0].bool ? -zoom : zoom)
                let y = p.y * (camera["flipped"][1].bool ? -zoom : zoom)
                let surface = CGPoint(x: CGFloat(camera["translation"][0].number) + cos(angle) * x - sin(angle) * y,
                    y: CGFloat(camera["translation"][1].number) + sin(angle) * x + cos(angle) * y)
                try require(surface.x >= 0 && surface.y >= 0
                    && surface.x < CGFloat(camera["viewport"][0].number)
                    && surface.y < CGFloat(camera["viewport"][1].number), "Supplied artwork contact must lie within the published viewport")
                return CGPoint(x: surface.x / canvas.contentScaleFactor, y: surface.y / canvas.contentScaleFactor)
            }
            touch.point = try position(points[0]); canvas.touchesBegan([touch], with: event)
            for (index, point) in points.enumerated().dropFirst() {
                touch.pressure = pressures.isEmpty ? 1 : pressures[index]
                touch.point = try position(point); touch.time = max(touch.time.nextUp, ProcessInfo.processInfo.systemUptime)
                canvas.touchesMoved([touch], with: event)
            }
            touch.time = max(touch.time.nextUp, ProcessInfo.processInfo.systemUptime)
            if cancel { canvas.touchesCancelled([touch], with: event) }
            else { canvas.touchesEnded([touch], with: event) }
            try await flush()
        }
        func pixels() async throws -> Data {
            try require(canvas.contacts.isEmpty, "Finish supplied contacts before capturing artwork")
            let prepared = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try require(prepared, "Prepare completed artwork for export")
            try await flush()
            try? FileManager.default.removeItem(at: png)
            try await invoke("export_document")
            try await wait("Finish PNG export") { !store.projectFiles.busy && FileManager.default.fileExists(atPath: png.path) }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            guard let source = CGImageSourceCreateWithURL(png as CFURL, nil),
                let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw HostFailure(message: "Exported PNG") }
            try require(image.width == 128 && image.height == 128, "Export the drawing, not the viewport")
            var bytes = Data(count: 128 * 128 * 4)
            bytes.withUnsafeMutableBytes { buffer in
                let context = CGContext(data: buffer.baseAddress, width: 128, height: 128, bitsPerComponent: 8,
                    bytesPerRow: 128 * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue)!
                context.draw(image, in: CGRect(x: 0, y: 0, width: 128, height: 128))
            }
            return bytes
        }
        func sample(_ bytes: Data, _ x: Int, _ y: Int) -> Data { bytes.subdata(in: (y * 128 + x) * 4..<(y * 128 + x + 1) * 4) }
        func colored(_ bytes: Data, _ x: Int, _ y: Int, red: Bool = false) -> Bool {
            let p = sample(bytes, x, y)
            return red ? Int(p[0]) > Int(p[2]) + 50 : Int(p[2]) > Int(p[0]) + 50
        }
        func history(_ paper: Data, _ painted: Data) async throws {
            try require(painted != paper, "The contact must change actual artwork")
            try await invoke("undo")
            try require(try await pixels() == paper, "One Undo must restore every original pixel")
            try await invoke("redo")
            try require(try await pixels() == painted, "Redo must restore every painted pixel")
            try await invoke("undo")
        }
        try await newDocument()
        try await action(["type":"set_color", "rgba":[1,0,0,1]])
        try await action(["type":"color", "action":["op":"swap"]])
        try await action(["type":"set_color", "rgba":[0,0,1,1]])
        try await action(["type":"set_brush_size", "value":4])
        let paper = try await pixels()
        if !vectorsOnly {
        let scroll = Scroll(), scrollEvent = ContactEvent(); scrollEvent.nativeType = .scroll
        func sameCamera(_ before: JSON) -> Bool {
            ["translation", "zoom", "rotation"].allSatisfy {
                store.state["camera"][$0].stableKey == before[$0].stableKey
            }
        }
        func near(_ a: Double, _ b: Double) -> Bool { abs(a - b) < 0.005 }
        let density = Double(canvas.contentScaleFactor)
        for cancelled in [false, true] {
            try await invoke("fit_canvas")
            let touch = Contact(), event = ContactEvent(); event.buttons = .button(3)
            canvas.touchesBegan([touch], with: event); try await flush()
            try require(canvas.gestureRecognizer(scroll, shouldReceive: scrollEvent),
                "UIKit must deliver scrolling while a middle-button contact is held")
            scrollEvent.nativeType = .transform
            try require(!canvas.gestureRecognizer(scroll, shouldReceive: scrollEvent), "Captured contacts must still exclude native transforms")
            scrollEvent.nativeType = .scroll
            var before = store.state["camera"]
            scroll.flags = .control; scroll.delta = CGPoint(x: 0, y: 40)
            canvas.scrolled(scroll); try await flush()
            try require(store.state["camera"]["zoom"].number > before["zoom"].number,
                "A held middle button must admit Control-scroll zoom")
            before = store.state["camera"]
            scroll.flags = []; scroll.delta = CGPoint(x: 3, y: 4)
            canvas.scrolled(scroll); try await flush()
            try require(near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 3 * density)
                && near(store.state["camera"]["translation"][1].number - before["translation"][1].number, 4 * density),
                "A held middle button must admit normal scroll")
            before = store.state["camera"]
            touch.time += 0.01; canvas.touchesMoved([touch], with: event); try await flush()
            try require(sameCamera(before), "Stationary middle-button motion must preserve the scrolled camera")
            touch.point.x += 5; touch.point.y += 7; touch.time += 0.01
            canvas.touchesMoved([touch], with: event); try await flush()
            try require(near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 5 * density)
                && near(store.state["camera"]["translation"][1].number - before["translation"][1].number, 7 * density),
                "Middle-button dragging must continue from the scrolled camera")
            touch.time += 0.01; event.buttons = []
            if cancelled { canvas.touchesCancelled([touch], with: event) }
            else { canvas.touchesEnded([touch], with: event) }
            try await flush()
            try require(canvas.contacts.isEmpty, "Middle-button release or cancellation must retire the native contact")
            before = store.state["camera"]
            scroll.delta = CGPoint(x: 3, y: 4); canvas.scrolled(scroll); try await flush()
            try require(!sameCamera(before), "Scroll must resume after middle-button release or cancellation")
        }
        try await invoke("fit_canvas")
        try require(try await pixels() == paper, "Middle-button navigation must preserve every artwork pixel")
        try await action(["type":"select_brush", "id":1])
        try await wait("Prepare navigation exclusion brush") { store.snapshot["brush_ready"].bool }
        let pinch = NativePinch(), rotation = NativeRotation()
        func recalledCamera(_ before: JSON) -> Bool {
            near(store.state["camera"]["zoom"].number, before["zoom"].number)
                && near(store.state["camera"]["rotation"].number, before["rotation"].number)
                && store.state["camera"]["flipped"].stableKey == before["flipped"].stableKey
                && (0..<2).allSatisfy { near(store.state["camera"]["translation"][$0].number, before["translation"][$0].number) }
        }
        func pinched(_ phase: UIGestureRecognizer.State, scale: CGFloat = 1) async throws {
            pinch.phase = phase; pinch.scale = scale
            canvas.pinched(pinch); try await flush()
        }
        func rotated(_ phase: UIGestureRecognizer.State, angle: CGFloat = 0) async throws {
            rotation.phase = phase; rotation.rotation = angle
            canvas.rotated(rotation); try await flush()
        }
        func recall(_ label: String, _ before: JSON, completed: JSON) async throws {
            try await invoke("previous_view")
            try require(recalledCamera(before), "\(label): Previous View must recall the beginning of the entire UIKit gesture; expected=\(before.stableKey), actual=\(store.state["camera"].stableKey)")
            try await invoke("previous_view")
            try require(recalledCamera(completed), "\(label): Previous View must return to the completed UIKit gesture; expected=\(completed.stableKey), actual=\(store.state["camera"].stableKey)")
        }
        try await path([CGPoint(x: 24, y: 64), CGPoint(x: 104, y: 64)], device: .pencil)
        let gesturePainted = try await pixels()
        try require(gesturePainted != paper, "The UIKit gesture checks must retain authored artwork")
        for theme in ["light", "dark"] {
            try await action(["type": "set_theme", "theme": theme])
            try await action(["type": "preferences", "action": ["type": "edit", "id": "wheel_behavior", "value": 1]])
            var before = store.state["camera"]
            scroll.flags = []; scroll.delta = CGPoint(x: 0, y: 40)
            canvas.scrolled(scroll); try await flush()
            try require(store.state["camera"]["zoom"].number > before["zoom"].number
                && near(store.state["camera"]["rotation"].number, before["rotation"].number),
                "UIKit Wheel Zoom must change zoom without rotating in \(theme)")
            before = store.state["camera"]
            scroll.flags = .control; scroll.delta = CGPoint(x: 0, y: 40)
            canvas.scrolled(scroll); try await flush()
            try require(store.state["camera"]["zoom"].number > before["zoom"].number,
                "Control retains UIKit Wheel Zoom in \(theme)")
            before = store.state["camera"]
            scroll.flags = .shift; scroll.delta = CGPoint(x: 3, y: 4)
            canvas.scrolled(scroll); try await flush()
            try require(near(store.state["camera"]["zoom"].number, before["zoom"].number)
                && near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 7 * density)
                && near(store.state["camera"]["translation"][1].number, before["translation"][1].number),
                "Shift pans horizontally with UIKit Wheel Zoom in \(theme)")
            try await action(["type": "preferences", "action": ["type": "edit", "id": "wheel_behavior", "value": 0]])
            try await action(["type": "preferences", "action": ["type": "edit", "id": "rotate_with_two_fingers", "value": false]])
            before = store.state["camera"]
            try await rotated(.began); try await rotated(.changed, angle: .pi / 9); try await rotated(.ended)
            try require(sameCamera(before), "Disabled UIKit two-finger rotation must preserve the entire camera in \(theme)")
            try await pinched(.began); try await pinched(.changed, scale: 1.1); try await pinched(.ended)
            try require(store.state["camera"]["zoom"].number > before["zoom"].number
                && near(store.state["camera"]["rotation"].number, before["rotation"].number),
                "Disabling rotation must preserve UIKit pinch zoom in \(theme)")
            try await action(["type": "preferences", "action": ["type": "edit", "id": "rotate_with_two_fingers", "value": true]])
            let left = Contact(), right = Contact(), touchEvent = ContactEvent()
            left.device = .direct; right.device = .direct
            for ratio in [1.6, 0.7, 1.2, 0.8] {
                let before = store.state["camera"]
                left.point = CGPoint(x: 440, y: 400); right.point = CGPoint(x: 760, y: 400)
                left.time += 0.1; right.time = left.time
                canvas.touchesBegan([left, right], with: touchEvent); try await flush()
                left.point.x = 600 - 160 * ratio; right.point.x = 600 + 160 * ratio
                left.time += 0.2; right.time = left.time
                canvas.touchesMoved([left, right], with: touchEvent); try await flush()
                left.time += 0.2; right.time = left.time
                canvas.touchesEnded([left, right], with: touchEvent); try await flush()
                try require(near(store.state["camera"]["zoom"].number, before["zoom"].number * ratio)
                    && canvas.contacts.isEmpty && canvas.ignoredContacts.isEmpty,
                    "Each fresh direct-touch pair must pinch and release both reused identities in \(theme): ratio=\(ratio), zoom=\(before["zoom"].number)->\(store.state["camera"]["zoom"].number), viewport=\(store.state["camera"]["viewport"]), work_area=\(store.state["camera"]["work_area"]), contacts=\(canvas.contacts.count), ignored=\(canvas.ignoredContacts.count)")
            }
            try await invoke("fit_canvas")
            try require(try await pixels() == gesturePainted, "Consecutive direct-touch pinches preserve every authored pixel")
            for mode in ["pinch", "rotation", "combined"] {
                let beginning = store.state["camera"]
                if mode != "rotation" {
                    try await pinched(.began)
                    try await pinched(.changed, scale: 1.25)
                }
                if mode != "pinch" {
                    try await rotated(.began)
                    try await rotated(.changed, angle: .pi / 9)
                }
                if mode != "rotation" {
                    try await pinched(.changed, scale: 1.10)
                    try await pinched(.ended)
                }
                if mode != "pinch" {
                    try await rotated(.changed, angle: .pi / 18)
                    try await rotated(.ended)
                }
                let completed = store.state["camera"]
                try require(!recalledCamera(beginning), "The supplied UIKit \(mode) gesture must move the view")
                try await recall("\(theme) \(mode)", beginning, completed: completed)
            }
            for interruption in ["cancel", "blur"] {
                try await pinched(.began)
                try await pinched(.changed, scale: 1.10)
                if interruption == "cancel" { try await pinched(.cancelled) }
                else { canvas.interruptContacts(); store.input(["type": "blur"]); try await flush() }
                try require(canvas.indirectGestures.isEmpty, "\(theme) \(interruption): native interruption must retire indirect recognizers")
                let resumed = store.state["camera"]
                try await rotated(.began)
                try await rotated(.changed, angle: .pi / 18)
                try await rotated(.changed, angle: .pi / 36)
                try await rotated(.ended)
                try await recall("\(theme) after \(interruption)", resumed, completed: store.state["camera"])
            }
            try await tool("select")
            for device: UITouch.TouchType in [.indirectPointer, .pencil] {
                try await invoke("fit_canvas"); try await invoke("zoom_out")
                let earlierRecall = store.state["camera"]
                try await invoke("fit_canvas")
                let rejectedBegin = store.state["camera"]
                try require(!recalledCamera(earlierRecall), "The old recall view must differ from the first accepted UIKit transform view")
                let contact = Contact(), event = ContactEvent(); contact.device = device
                canvas.touchesBegan([contact], with: event); try await flush()
                try await pinched(.began); try await rotated(.began)
                try require(sameCamera(rejectedBegin), "Rejected transform beginnings must preserve the active \(device) contact view")
                contact.time += 0.01; canvas.touchesEnded([contact], with: event); try await flush()
                try require(canvas.contacts.isEmpty, "The stationary \(device) selection contact must release normally")
                try require(try await pixels() == gesturePainted, "The released selection contact must preserve every artwork pixel")
                let firstAccepted = store.state["camera"]
                try require(recalledCamera(rejectedBegin), "The released contact must preserve the transform's starting view")
                try await pinched(.changed, scale: 1.10)
                try await rotated(.changed, angle: .pi / 18)
                try await pinched(.ended)
                try await rotated(.changed, angle: .pi / 36)
                try await rotated(.ended)
                try await recall("\(theme) after rejected \(device) beginning", firstAccepted, completed: store.state["camera"])
            }
            try await action(["type": "select_brush", "id": 1])
            try await wait("Restore the navigation exclusion brush") { store.snapshot["brush_ready"].bool }
            try require(try await pixels() == gesturePainted, "UIKit recall must preserve every artwork pixel in \(theme)")
        }
        for device: UITouch.TouchType in [.indirectPointer, .pencil, .direct] {
            let touch = Contact(), event = ContactEvent(); touch.device = device
            canvas.touchesBegan([touch], with: event); try await flush()
            try require(canvas.gestureRecognizer(scroll, shouldReceive: scrollEvent), "Scroll admission must let shared Rust decide primary contact exclusion")
            let before = store.state["camera"]
            for flags: UIKeyModifierFlags in [[], .control] {
                scroll.flags = flags; scroll.delta = CGPoint(x: 3, y: 40)
                canvas.scrolled(scroll); try await flush()
                try require(sameCamera(before), "Scroll must preserve the camera during a primary \(device) contact")
            }
            try await pinched(.began); try await pinched(.changed, scale: 1.25); try await pinched(.ended)
            try await rotated(.began); try await rotated(.changed, angle: .pi / 6); try await rotated(.ended)
            try require(sameCamera(before), "Indirect transforms must preserve the camera during a primary \(device) contact")
            touch.time += 0.01; canvas.touchesCancelled([touch], with: event); try await flush()
        }
        let resumed = store.state["camera"]
        try await pinched(.began)
        try await pinched(.changed, scale: 1.10)
        try await pinched(.changed, scale: 1.05)
        try await pinched(.ended)
        try await recall("after captured-contact cancellation", resumed, completed: store.state["camera"])
        try require(try await pixels() == gesturePainted, "Captured-contact exclusion and resumed recall must preserve every pixel")
        try await history(paper, gesturePainted)
        try await newDocument()
        passed("PASS: UIKit middle-button scrolling, indirect gesture grouping/recall, interruption, primary contact exclusion and exact artwork history")
        for device: UITouch.TouchType in [.indirectPointer, .pencil] {
            for shape in ["line", "rectangle", "ellipse"] {
                for paint in shape == "line" ? ["outline"] : ["outline", "fill", "both"] {
                    for shift in [false, true] {
                        try await tool(["figure":["shape":shape, "paint":paint]])
                        let points = [CGPoint(x:24,y:24), CGPoint(x:60,y:46), CGPoint(x:96,y:68)]
                        try await path(points, device: device, modifiers: shift ? .shift : [], cancel: true)
                        try require(try await pixels() == paper, "Cancelled figure must preserve every pixel")
                        try await path(points, device: device, modifiers: shift ? .shift : [])
                        let painted = try await pixels()
                        let center = shift ? 60 : 46
                        if shape == "line" {
                            try require(colored(painted, shift ? 50 : 60, shift ? 50 : 46), "Figure line follows its constraint")
                        } else {
                            try require(colored(painted, 60, 24), "Figure top edge uses foreground color")
                            if paint == "outline" {
                                try require(sample(painted, 60, center) == sample(paper, 60, center), "Outline preserves its interior")
                            } else {
                                try require(colored(painted, 60, center, red: paint == "both"), "Figure fill color")
                            }
                            if shift { try require(colored(painted, 60, 95), "Shift makes the shape square or circular") }
                        }
                        try require(sample(painted, 115, 115) == sample(paper, 115, 115), "Figures preserve outside pixels")
                        try await history(paper, painted)
                        passed("PASS: \(device) \(shape) \(paint), Shift=\(shift), cancellation and exact PNG history")
                    }
                }
            }
            for radial in [false, true] {
                for transparent in [false, true] {
                    try await tool(["gradient":["shape":radial ? "Radial" : "Linear"]])
                    guard let control = store.state["tool_extra"].array.first(where: { !$0["Gradient"].isNull }) else {
                        throw HostFailure(message: "Published gradient tool definition")
                    }
                    let destination = control["Gradient"]["gradient"]["destination"]
                    let stops: [[String: Any]] = [
                        ["position":0, "color":["space":"Srgb", "rgba":[0,0,1,1]]],
                        ["position":1, "color":["space":"Srgb", "rgba":[1,0,0,transparent ? 0 : 1]]],
                    ]
                    for (index, stop) in stops.enumerated() {
                        try await action(["type":"effect", "action":["op":"gradient", "target":destination.object,
                            "edit":["kind":"stop", "index":index, "position":stop["position"]!, "color":stop["color"]!, "remove":false]]])
                    }
                    let points = [CGPoint(x:32,y:64), CGPoint(x:64,y:64), CGPoint(x:96,y:64)]
                    try await path(points, device: device, cancel: true)
                    try require(try await pixels() == paper, "Cancelled gradient preserves every pixel")
                    try await path(points, device: device)
                    let painted = try await pixels()
                    try require(colored(painted, 32, 64) && sample(painted, 32, 64) != sample(painted, 64, 64), "Gradient begins in foreground and varies across the drawing")
                    try require(transparent ? sample(painted, 116, 64) == sample(paper, 116, 64) : colored(painted, 116, 64, red: true), "Gradient reaches its transparent/background endpoint")
                    try await history(paper, painted)
                    passed("PASS: \(device) gradient radial=\(radial), transparent=\(transparent), cancellation and exact PNG history")
                }
            }
            for kind in ["straight", "parallel", "radial"] {
                try await newDocument()
                try await tool(["ruler":["kind":kind]])
                try await path([CGPoint(x:24,y:64), CGPoint(x:64,y:64), CGPoint(x:104,y:64)], device: device)
                try require(try await pixels() == paper, "Ruler creation must not paint")
                try await action(["type":"select_brush", "id":1])
                try await action(["type":"set_brush_size", "value":3])
                try await wait("Prepare ruler brush") { store.snapshot["brush_ready"].bool }
                let points = [CGPoint(x:32,y:64), CGPoint(x:48,y:88), CGPoint(x:64,y:84), CGPoint(x:80,y:80), CGPoint(x:96,y:84)]
                try await path(points, device: device)
                let snapped = try await pixels()
                let ys = (0..<128).filter { y in (20..<110).contains { x in colored(snapped,x,y) } }
                try require(!ys.isEmpty && ys.allSatisfy { abs($0 - 64) <= 4 }, "\(kind) ruler constrains UIKit painting")
                try await history(paper, snapped)
                try await invoke("snap_rulers")
                try await path(points, device: device)
                let free = try await pixels()
                try require((72..<96).contains { y in (32..<104).contains { x in colored(free,x,y) } }, "Disabling snapping restores free painting")
                try await history(paper, free)
                try await invoke("snap_rulers")
                passed("PASS: \(device) \(kind) ruler, constrained/free painting and exact PNG history")
            }
            try await newDocument()
            try await action(["type":"set_brush_size", "value":4])
            try await tool(["figure":["shape":"rectangle", "paint":"fill"]])
            try await path([CGPoint(x:32,y:32), CGPoint(x:96,y:96)], device: device)
            let original = try await pixels()
            try require(colored(original, 64, 64) && !colored(original, 20, 20), "Transform fixture contains finite artwork")
            struct TransformCase {
                let name: String
                let start: CGPoint
                let end: CGPoint
                let flags: UIKeyModifierFlags
                let pose: [Double]
                var rotation = false
            }
            var cases: [TransformCase] = [
                .init(name:"edge", start:CGPoint(x:96,y:64), end:CGPoint(x:80,y:64), flags:[], pose:[56,64,0.75,1,0]),
                .init(name:"Shift edge", start:CGPoint(x:96,y:64), end:CGPoint(x:80,y:64), flags:.shift, pose:[56,64,0.75,0.75,0]),
                .init(name:"Alt edge", start:CGPoint(x:96,y:64), end:CGPoint(x:80,y:64), flags:.alternate, pose:[64,64,0.5,1,0]),
                .init(name:"corner", start:CGPoint(x:96,y:96), end:CGPoint(x:80,y:88), flags:[], pose:[56,60,0.75,0.875,0]),
                .init(name:"Shift corner", start:CGPoint(x:96,y:96), end:CGPoint(x:80,y:88), flags:.shift, pose:[56,56,0.75,0.75,0]),
                .init(name:"Alt corner", start:CGPoint(x:96,y:96), end:CGPoint(x:80,y:88), flags:.alternate, pose:[64,64,0.5,0.75,0]),
                .init(name:"move", start:CGPoint(x:48,y:48), end:CGPoint(x:64,y:56), flags:[], pose:[80,72,1,1,0]),
                .init(name:"Shift move", start:CGPoint(x:48,y:48), end:CGPoint(x:64,y:56), flags:.shift, pose:[80,64,1,1,0])
            ]
            let radius = 8 + 30 / store.state["camera"]["zoom"].number
            let angle = 71.0 * Double.pi / 180
            for shift in [false, true] {
                cases.append(.init(name:shift ? "Shift rotation" : "rotation",
                    start:CGPoint(x:64,y:64-radius), end:CGPoint(x:64+sin(angle)*radius,y:64-cos(angle)*radius),
                    flags:shift ? .shift : [], pose:[64,64,0.5,0.25,(shift ? 75 : 71) * Double.pi / 180], rotation:true))
            }
            for test in cases {
                func begin() async throws {
                    try await invoke("scale_rotate")
                    try await wait("Prepare transform controls") {
                        store.state["tool_settings"].array.contains { $0["id"].string == "transform_x" }
                            && store.snapshot["brush_ready"].bool
                    }
                    if test.rotation {
                        try await action(["type":"set_tool_setting", "id":"transform_width", "value":0.5])
                        try await action(["type":"set_tool_setting", "id":"transform_height", "value":0.25])
                    }
                    try await wait("Prepare transform preview") { store.snapshot["brush_ready"].bool }
                }
                func expectPose() throws {
                    for (id, expected) in zip(["x", "y", "width", "height", "angle"], test.pose) {
                        let control = store.state["tool_settings"].array.first { $0["id"].string == "transform_" + id }
                        try require(control != nil && abs(control!["value"].number - expected) < 0.002,
                            "\(device) \(test.name) \(id): expected \(expected), got \(control?["value"].number ?? -999)")
                    }
                }
                try await begin()
                let initialControls = store.state["tool_settings"].stableKey
                try await path([test.start,test.end], device:device, modifiers:test.flags, cancel:true)
                try await wait("Cancelled contact restores the transform's starting pose") {
                    store.state["tool_settings"].stableKey == initialControls && canvas.contacts.isEmpty
                }
                try await invoke("cancel_transform")
                try require(try await pixels() == original, "Cancelled transform contact preserves every pixel")
                for apply in [false, true] {
                    try await begin()
                    try await path([test.start,test.end], device:device, modifiers:test.flags)
                    try expectPose()
                    try await invoke(apply ? "apply_transform" : "cancel_transform")
                    let changed = try await pixels()
                    if apply {
                        try require(colored(changed,64,64), "Transformed artwork retains its expected center")
                        if test.rotation {
                            try require(colored(changed,64,54) && !colored(changed,54,64), "Rotation changes actual artwork orientation")
                        } else if test.name.contains("move") {
                            try require(!colored(changed,40,64) && colored(changed,104,64), "Moving artwork updates both old and new positions")
                        } else {
                            try require(!colored(changed,88,64), "Scaling moves the original right edge inward")
                            if test.flags.contains(.alternate) { try require(!colored(changed,40,64), "Alt scaling keeps the center fixed") }
                            else { try require(colored(changed,40,64), "Ordinary scaling keeps the opposite edge fixed") }
                        }
                        try await history(original, changed)
                    } else {
                        try require(changed == original, "Cancel restores every original pixel")
                        try await invoke("undo")
                        try require(try await pixels() == paper, "Cancelled transforms must not add artwork history")
                        try await invoke("redo")
                        try require(try await pixels() == original, "Original artwork Redo survives cancelled transforms")
                    }
                }
                passed("PASS: \(device) transform \(test.name), native handle pose, contact/command cancellation, Apply and exact PNG history")
            }
            try await newDocument()
        }
        let pressurePoints: [[Double]] = [[0, 0], [1, 1]]
        try await action(["type":"restore_settings", "settings":store.state["settings"]
            .replacing("pressure_curve", with: JSON(pressurePoints)).raw])
        try await wait("Apply linear calibration for supplied dry-media pressures") {
            store.state["settings"]["pressure_curve"].array.map { $0.array.map(\.number) } == pressurePoints
        }
        let dryPresets = [(2, "Pencil"), (25, "Pointy Pencil"), (26, "Shading Pencil"),
            (6, "Chalk"), (17, "Pastel Block"), (27, "Charcoal"), (43, "Crayon")]
        let dryPoses: [(String, CGFloat)] = [("upright", 0), ("writing", 0.5), ("side", 0.85)]
        let dryPressures: [CGFloat] = [0, 0.15, 0.45, 1]
        let dryPoints = dryPressures.indices.map { CGPoint(x: 24 + CGFloat($0) * 80 / 3, y: 64) }
        let contactRows = 40..<88
        let outsidePixelCount = (0..<128).filter { !contactRows.contains($0) }.count * 128
        try require(outsidePixelCount > 0, "The dry contact oracle must retain outside pixels")
        for theme in ["light", "dark"] {
            try await action(["type":"set_theme", "theme":theme])
            try require(store.state["theme"].string == theme, "Apply the supplied dry-media theme")
            for (preset, name) in dryPresets {
                try await newDocument()
                try await action(["type":"select_brush", "id":preset])
                try await action(["type":"set_color", "rgba":[0,0,1,1]])
                try await action(["type":"set_brush_size", "value":24])
                try await wait("Prepare dry-media preset \(name)") {
                    store.state["brush"]["preset"].uint == UInt64(preset)
                        && store.state["brush"]["diameter"].number == 24 && store.snapshot["brush_ready"].bool
                }
                try require(store.state["camera"]["rotation"].number == 0
                    && store.state["camera"]["flipped"].array.allSatisfy { !$0.bool }, "Dry contact bounds require the default document orientation")
                let dryPaper = try await pixels()
                for (pose, tilt) in dryPoses {
                    let altitude = CGFloat.pi / 2 * (1 - tilt)
                    let label = "\(name) \(theme) \(pose)"
                    try require(try await pixels() == dryPaper, "\(label): start on exact blank paper")
                    try await path(dryPoints, device:.pencil, pressures:Array(repeating:0, count:dryPoints.count), altitude:altitude)
                    try require(try await pixels() == dryPaper, "\(label): zero supplied pressure must preserve every pixel")
                    try await path(dryPoints, device:.pencil, pressures:dryPressures, altitude:altitude)
                    try require(canvas.contacts.isEmpty && canvas.estimates.pending.isEmpty,
                        "\(label): completed supplied Pencil contact must retire its observations")
                    let painted = try await pixels()
                    try require((40..<88).contains { y in (8..<120).contains { x in
                        let pixel = sample(painted,x,y)
                        return Int(pixel[2]) > Int(pixel[0]) + 10 && pixel != sample(dryPaper,x,y)
                    } }, "\(label): pressure sweep must deposit blue pigment in its authored path region")
                    try require((0..<128*128).allSatisfy { pixel in
                        let x = pixel % 128, y = pixel / 128
                        return contactRows.contains(y) || sample(painted,x,y) == sample(dryPaper,x,y)
                    }, "\(label): preserve every pixel outside the bounded contact region")
                    try await history(dryPaper, painted)
                    try require(try await pixels() == dryPaper, "\(label): final Undo restores every original pixel")
                    passed("PASS: supplied UIKit Pencil \(label), zero force, pressure sweep, bounded pigment and exact PNG history")
                }
            }
        }
        }
        func vectorLayer() async throws -> UInt64 {
            try await newDocument()
            try await action(["type":"select_brush", "id":2])
            try require(store.state["brush"]["preset"].uint == 2, "Prepare Pencil before discovering a vector layer")
            try await wait("Admit New Vector") { store.command("new_vector_layer")["enabled"].bool }
            try await invoke("new_vector_layer")
            try await wait("New Vector must publish a vector row") {
                let layer = store.state["layer_tools"]["editing_layer"]["id"].uint
                return store.state["layers"].array.contains {
                    $0["id"].uint == layer && $0["content_icon"].string == "layer-path-symbolic"
                }
            }
            let layer = store.state["layer_tools"]["editing_layer"]["id"].uint
            try require(store.state["brush"]["preset"].uint == 1, "New Vector must select G-Pen")
            try await action(["type":"set_color", "rgba":[0,0,1,1]])
            try await action(["type":"set_brush_size", "value":4])
            try await wait("Prepare vector G-Pen") { store.snapshot["brush_ready"].bool }
            return layer
        }
        func property(_ layer: UInt64, _ key: String, kind: String, value: Any) async throws {
            try require(store.state["layer_properties"]["controls"].array.contains { $0["key"].string == key },
                "Selected paths must expose \(key) in Properties")
            try await action(["type":"effect", "action":["op":"set", "layer":layer, "key":key,
                "value":["kind":kind, "value":value]]])
        }
        for theme in ["light", "dark"] {
            try await action(["type":"set_theme", "theme":theme])
            let layer = try await vectorLayer()
            try await path([CGPoint(x:24,y:24), CGPoint(x:40,y:24), CGPoint(x:56,y:24)], device:.pencil)
            let line = try await pixels()
            try require(colored(line,40,24) && !colored(line,40,40), "Supplied Pencil must ink the vector path only")
            try await history(paper,line)
            try await invoke("redo")
            try await invoke("move")
            try await path([CGPoint(x:40,y:24), CGPoint(x:40,y:32), CGPoint(x:40,y:40)], device:.pencil)
            let moved = try await pixels()
            try require(colored(moved,40,40) && !colored(moved,40,24), "Move must relocate the selected vector path")
            try await history(line,moved)
            try await invoke("redo")
            try await invoke("select_all")
            try await property(layer,"path_outline_color",kind:"color",value:["space":"Srgb","rgba":[1,0,0,1]])
            let restyled = try await pixels()
            try require(colored(restyled,40,40,red:true) && !colored(restyled,40,40), "Properties must recolor the selected path")
            try await history(moved,restyled)
            try await invoke("redo")
            try await invoke("select_all")
            try await property(layer,"path_width",kind:"number",value:12)
            let widened = try await pixels()
            try require(colored(widened,40,44,red:true) && !colored(restyled,40,44,red:true), "Properties must widen the selected path")
            try await history(restyled,widened)
            passed("PASS: supplied UIKit Pencil vector ink, Move, Properties color/width and exact PNG history in \(theme)")

            _ = try await vectorLayer()
            try await tool(["figure":["shape":"rectangle","paint":"fill"]])
            try await path([CGPoint(x:16,y:16),CGPoint(x:80,y:80)],device:.pencil)
            try await action(["type":"set_color","rgba":[1,0,0,1]])
            try await path([CGPoint(x:48,y:48),CGPoint(x:112,y:112)],device:.pencil)
            let front = try await pixels()
            try require(colored(front,64,64,red:true) && colored(front,32,32), "New vector figures must overlap in their authored order")
            try await invoke("send_to_back")
            let back = try await pixels()
            try require(colored(back,64,64) && colored(back,96,96,red:true), "Arrange must restack the selected path within its vector layer")
            try await history(front,back)
            try await invoke("redo")
            try await invoke("bring_to_front")
            try require(try await pixels() == front, "Bring to Front must restore the exact original path order")
            passed("PASS: supplied UIKit Pencil vector figures, Arrange and exact PNG history in \(theme)")

            _ = try await vectorLayer()
            try await tool(["figure":["shape":"path","paint":"outline"]])
            for point in [CGPoint(x:24,y:96),CGPoint(x:48,y:112)] { try await path([point,point],device:.pencil) }
            try require(store.command("finish_path")["enabled"].bool, "Path must publish its Finish action while drafting")
            try await invoke("cancel_path")
            let cancelledPath = try await pixels()
            try require(!store.command("finish_path")["enabled"].bool && cancelledPath == paper,
                "Cancel Path must retire the draft without changing artwork")
            for point in [CGPoint(x:24,y:24),CGPoint(x:64,y:24),CGPoint(x:104,y:64)] { try await path([point,point],device:.pencil) }
            try await invoke("finish_path")
            let openPath = try await pixels()
            try require(colored(openPath,44,24) && !colored(openPath,44,36), "Finish Path must commit the supplied anchors")
            try await invoke("edit_points")
            try require(store.state["layer_tools"]["tool"].string == "edit_points", "Edit Points must become the active tool")
            try await path([CGPoint(x:24,y:24),CGPoint(x:24,y:36),CGPoint(x:24,y:48)],device:.pencil)
            let edited = try await pixels()
            try require(colored(edited,44,36) && !colored(edited,44,24), "Edit Points must move an anchor without moving the other anchors")
            try await history(openPath,edited)
            try await invoke("rasterize_layer")
            try require(try await pixels() == openPath, "Rasterize Layer must keep every vector pixel")
            try require(!store.command("rasterize_layer")["enabled"].bool, "Rasterize Layer must replace the vector content with paint")
            try await invoke("undo")
            let restoredPath = try await pixels()
            try require(store.command("rasterize_layer")["enabled"].bool && restoredPath == openPath,
                "One Undo must restore editable vector content without changing its picture")
            passed("PASS: supplied UIKit Pencil Path draft/cancel/finish, Edit Points, Rasterize and exact PNG history in \(theme)")

            let maskedLayer = try await vectorLayer()
            try await action(["type":"set_brush_size","value":8])
            try await path([CGPoint(x:16,y:64),CGPoint(x:64,y:64),CGPoint(x:112,y:64)],device:.pencil)
            let unmasked = try await pixels()
            try await action(["type":"layer","action":["op":"add_mask","id":maskedLayer,"replace":false]])
            try await action(["type":"layer","action":["op":"select","id":maskedLayer,"mask":true]])
            try require(store.state["layer_tools"]["editing_layer"]["mask_selected"].bool, "The vector mask must become the drawing target")
            try await invoke("eraser")
            try await action(["type":"set_brush_size","value":24])
            try await wait("Prepare vector layer-mask eraser") { store.snapshot["brush_ready"].bool }
            try await path([CGPoint(x:64,y:32),CGPoint(x:64,y:64),CGPoint(x:64,y:96)],device:.pencil)
            let masked = try await pixels()
            try require(!colored(masked,64,64) && colored(masked,32,64) && colored(masked,96,64),
                "Painting the vector mask must hide only the crossed part of its path")
            try await history(unmasked,masked)
            try await invoke("redo")
            try await action(["type":"layer","action":["op":"select","id":maskedLayer,"mask":false]])
            try? FileManager.default.removeItem(at:vectorPackage)
            try await invoke("save_document_as")
            try await wait("Save masked vector package") { !store.projectFiles.busy && FileManager.default.fileExists(atPath:vectorPackage.path) }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "Save masked vector package")
            try await newDocument()
            let identity = store.drawingIdentity
            store.projectFiles.openURL(vectorPackage)
            try await wait("Reopen masked vector package") { !store.projectFiles.busy && store.drawingIdentity != identity }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "Reopen masked vector package")
            try require(try await pixels() == masked, "Save and reopen must preserve every masked vector pixel")
            guard let reopened = store.state["layers"].array.first(where: { $0["content_icon"].string == "layer-path-symbolic" }) else {
                throw HostFailure(message:"Reopened package must retain editable vector content")
            }
            try require(reopened["has_mask"].bool, "Reopened vector layer must retain its authored mask")
            try await action(["type":"layer","action":["op":"select","id":reopened["id"].uint,"mask":false]])
            try await invoke("rasterize_layer")
            try require(try await pixels() == masked, "Rasterizing the reopened vector layer must preserve its masked picture")
            try await invoke("undo")
            let restoredMask = try await pixels()
            try require(store.command("rasterize_layer")["enabled"].bool && restoredMask == masked,
                "Undo must restore the reopened vector and its mask")
            passed("PASS: supplied UIKit Pencil vector masks, save/reopen, masked Rasterize and exact PNG history in \(theme)")
        }
        let closed = await withCheckedContinuation { done in store.prepareClose { done.resume(returning: $0) } }
        try require(closed, "Close private UIKit input storage")
    }
}

@main struct CanvasModifierChecks {
    static func main() {
        UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(ModifierChecks.self))
    }
}
