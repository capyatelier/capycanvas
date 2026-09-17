import UIKit
import ImageIO

// Supplied UIKit contacts through the production canvas and Rust owner. Saved
// ruler geometry checks the modifier's effect, without keyboard/UI automation.
private final class Contact: UITouch {
    var point = CGPoint(x: 500, y: 400)
    var time = ProcessInfo.processInfo.systemUptime
    var device: UITouch.TouchType = .indirectPointer
    override var type: UITouch.TouchType { device }
    override var timestamp: TimeInterval { time }
    override func preciseLocation(in view: UIView?) -> CGPoint { point }
    override var estimationUpdateIndex: NSNumber? { nil }
    override var estimatedPropertiesExpectingUpdates: UITouch.Properties { [] }
    override var force: CGFloat { 1 }
    override var maximumPossibleForce: CGFloat { 1 }
    override var altitudeAngle: CGFloat { .pi / 2 }
    override func azimuthAngle(in view: UIView?) -> CGFloat { 0 }
    override var rollAngle: CGFloat { 0 }
}
private final class ContactEvent: UIEvent {
    var flags: UIKeyModifierFlags = []
    override var modifierFlags: UIKeyModifierFlags { flags }
    override var buttonMask: UIEvent.ButtonMask { .primary }
    override func coalescedTouches(for touch: UITouch) -> [UITouch]? { nil }
    override func predictedTouches(for touch: UITouch) -> [UITouch]? { nil }
}

@MainActor private final class ModifierChecks: NSObject, UIApplicationDelegate, UIWindowSceneDelegate {
    var window: UIWindow?
    private var completedGroups = 0
    private let runID = ProcessInfo.processInfo.environment["CAPY_INPUT_RUN"] ?? UUID().uuidString
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Input Checks", sessionRole: session.role)
        configuration.delegateClass = ModifierChecks.self
        return configuration
    }
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        window.rootViewController = UIViewController()
        window.makeKeyAndVisible(); self.window = window
        Task { @MainActor in
            do {
                try await run(); try report(nil)
                print("PASS: UIKit contact modifiers and ruler history"); exit(0)
            } catch {
                try? report(error.localizedDescription)
                print("FAIL: \(error.localizedDescription)"); exit(1)
            }
        }
    }
    private func passed(_ message: String) { completedGroups += 1; print(message) }
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
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-modifiers-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("ruler.capy")
        let store = EditorStore(platform: 0,
            persistence: EditorPersistence(root: root.appendingPathComponent("state")), managedWorkspaces: false)
        let canvas = CanvasView(store: store)
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
                native.submit(2, JSON(["type":"catalog"])) { _ in done.resume() }
            }
            try require(store.failure == nil, store.failure ?? "")
        }
        func action(_ value: [String: Any]) async throws {
            store.dispatch(value); try await flush()
        }
        func rulers() async throws -> JSON {
            let prepared = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try require(prepared, "Prepare completed ruler input")
            try await flush()
            let file = store.state["document_file"]
            let task = try await withCheckedThrowingContinuation { (done: CheckedContinuation<NativeProjectTask, Error>) in
                native.recoveryTask(expected: (file["epoch"].uint, file["revision"].uint)) { task, error in
                    if let task { done.resume(returning: task) }
                    else { done.resume(throwing: HostFailure(message: error ?? "Prepare ruler archive")) }
                }
            }
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                NativeProjectTask.io.async {
                    do { try task.write(to: url); done.resume() }
                    catch { done.resume(throwing: error) }
                }
            }
            let bytes = try Data(contentsOf: url)
            try require(bytes.count >= 52 && bytes.prefix(12) == Data("CAPYRASTER\u{1}\0".utf8), "Ruler archive header")
            let count = bytes[12..<20].enumerated().reduce(UInt64(0)) { $0 | UInt64($1.element) << ($1.offset * 8) }
            try require(count <= bytes.count - 52, "Complete ruler archive")
            return JSON(try JSONSerialization.jsonObject(with: bytes.subdata(in: 52..<(52 + Int(count)))))["document"]["rulers"]
        }
        try await flush()
        store.cameraRevision = store.state["camera"]["revision"].uint
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
        try await artwork(store: store, canvas: canvas, root: root)
    }

    private func artwork(store: EditorStore, canvas: CanvasView, root: URL) async throws {
        let native = store.native!, png = root.appendingPathComponent("artwork.png")
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { _, done in done([]) }, save: { _, _, done in done(nil) },
            export: { staging, done in
                do { try FileManager.default.copyItem(at: staging, to: png); done(png) }
                catch { store.failure = error.localizedDescription; done(nil) }
            }, exportOptions: { $0.choose($0.recipe) }))
        func flush() async throws {
            // Live transform previews are intentionally not recoverable yet.
            // Drive the ordinary renderer frame rather than requesting a save.
            let now = FrameTrace.now()
            await withCheckedContinuation { done in
                native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() }
            }
            await withCheckedContinuation { done in native.submit(2, JSON(["type":"catalog"])) { _ in done.resume() } }
            try require(store.failure == nil, store.failure ?? "Prepare canvas work")
        }
        func wait(_ description: String, _ ready: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(30)
            while !ready() {
                try require(Date() < deadline, description)
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
            let task = try await withCheckedThrowingContinuation { (done: CheckedContinuation<NativeProjectTask, Error>) in
                native.projectTask(kind: .open) { task, error in
                    if let task { done.resume(returning: task) }
                    else { done.resume(throwing: HostFailure(message: error ?? "Prepare drawing")) }
                }
            }
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                NativeProjectTask.io.async {
                    do { try task.read(from: nil, options: JSON(["extent": [128, 128], "color": ["space": "Srgb", "depth": "U8"], "background": "White"])); done.resume() }
                    catch { done.resume(throwing: error) }
                }
            }
            let error = await withCheckedContinuation { done in
                native.finishProject(task, opening: true, title: "Canvas input check", url: nil) { done.resume(returning: $0) }
            }
            try require(error == nil, error ?? "")
            try await invoke("fit_canvas")
            try await wait("Prepare replacement drawing") { store.snapshot["brush_ready"].bool }
        }
        func path(_ points: [CGPoint], device: UITouch.TouchType, modifiers: UIKeyModifierFlags = [], cancel: Bool = false) async throws {
            let touch = Contact(), event = ContactEvent(); touch.device = device
            event.flags = modifiers
            let camera = store.state["camera"]
            func position(_ p: CGPoint) -> CGPoint {
                CGPoint(x: camera["translation"][0].number + p.x * camera["zoom"].number,
                    y: camera["translation"][1].number + p.y * camera["zoom"].number)
            }
            touch.point = position(points[0]); canvas.touchesBegan([touch], with: event)
            for point in points.dropFirst() {
                touch.point = position(point); touch.time = max(touch.time.nextUp, ProcessInfo.processInfo.systemUptime)
                canvas.touchesMoved([touch], with: event)
            }
            touch.time = max(touch.time.nextUp, ProcessInfo.processInfo.systemUptime)
            if cancel { canvas.touchesCancelled([touch], with: event) }
            else { canvas.touchesEnded([touch], with: event) }
            try await flush()
        }
        func pixels() async throws -> Data {
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
                    try await tool(["gradient":["radial":radial, "transparent":transparent]])
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
            // Values are shared document units: X/Y pixels, scale fractions,
            // and radians. Handles surround the finite 128px raster canvas.
            struct TransformCase {
                let name: String
                let start: CGPoint
                let end: CGPoint
                let flags: UIKeyModifierFlags
                let pose: [Double]
                var rotation = false
            }
            var cases: [TransformCase] = [
                .init(name:"edge", start:CGPoint(x:128,y:64), end:CGPoint(x:96,y:64), flags:[], pose:[-16,0,0.75,1,0]),
                .init(name:"Shift edge", start:CGPoint(x:128,y:64), end:CGPoint(x:96,y:64), flags:.shift, pose:[-16,0,0.75,0.75,0]),
                .init(name:"Alt edge", start:CGPoint(x:128,y:64), end:CGPoint(x:96,y:64), flags:.alternate, pose:[0,0,0.5,1,0]),
                .init(name:"corner", start:CGPoint(x:128,y:128), end:CGPoint(x:96,y:112), flags:[], pose:[-16,-8,0.75,0.875,0]),
                .init(name:"Shift corner", start:CGPoint(x:128,y:128), end:CGPoint(x:96,y:112), flags:.shift, pose:[-16,-16,0.75,0.75,0]),
                .init(name:"Alt corner", start:CGPoint(x:128,y:128), end:CGPoint(x:96,y:112), flags:.alternate, pose:[0,0,0.5,0.75,0]),
                .init(name:"move", start:CGPoint(x:64,y:64), end:CGPoint(x:80,y:72), flags:[], pose:[16,8,1,1,0]),
                .init(name:"Shift move", start:CGPoint(x:64,y:64), end:CGPoint(x:80,y:72), flags:.shift, pose:[16,0,1,1,0])
            ]
            let radius = 16 + 30 / store.state["camera"]["zoom"].number
            let angle = 71.0 * Double.pi / 180
            for shift in [false, true] {
                cases.append(.init(name:shift ? "Shift rotation" : "rotation",
                    start:CGPoint(x:64,y:64-radius), end:CGPoint(x:64+sin(angle)*radius,y:64-cos(angle)*radius),
                    flags:shift ? .shift : [], pose:[0,0,0.5,0.25,(shift ? 75 : 71) * Double.pi / 180], rotation:true))
            }
            for test in cases {
                func begin() async throws {
                    try await invoke("scale_rotate")
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
                try await path([test.start,test.end], device:device, modifiers:test.flags, cancel:true)
                try require(store.state["tool_settings"].array.isEmpty, "Cancelled contact retires the transform preview")
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
                            try require(colored(changed,64,48) && !colored(changed,48,64), "Rotation changes actual artwork orientation")
                        } else if test.name.contains("move") {
                            try require(!colored(changed,40,64) && colored(changed,104,64), "Moving artwork updates both old and new positions")
                        } else {
                            try require(!colored(changed,88,64), "Scaling moves the original right edge inward")
                            if test.flags.contains(.alternate) { try require(!colored(changed,40,64), "Alt scaling keeps the center fixed") }
                            else { try require(colored(changed,26,64), "Ordinary scaling keeps the opposite edge fixed") }
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
    }
}

@main struct CanvasModifierChecks {
    static func main() {
        UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(ModifierChecks.self))
    }
}
