import AppKit
import CryptoKit
import ImageIO
import SwiftUI

private struct RetainedProfileLibraryFixture: View {
    @ObservedObject var store: EditorStore
    let preferences: ColorPreferencesStore
    var body: some View {
        ColorProfileLibrary(preferences: preferences)
            .environment(\.capyInterfaceLanguage, store.interfaceLanguage)
            .environment(\.capyProfileCopy, store.catalog["profile_copy"])
            .environment(\.capyNativeCopy, store.catalog["native_copy"])
            .environment(\.colorScheme, store.state["theme"].string == "dark" ? .dark : .light)
    }
}

/// Public AppKit event values supplied to the real canvas callbacks. Pointer
/// contacts below still use the native queue; this does not emulate a trackpad.
private final class NavigationEvent: NSEvent {
    var point = CGPoint.zero
    var flags: NSEvent.ModifierFlags = []
    var delta = CGPoint.zero
    var precise = true
    var eventPhase: NSEvent.Phase = .changed
    var magnificationDelta: CGFloat = 0.25
    var rotationDelta: Float = 30
    override var phase: NSEvent.Phase { eventPhase }
    override var locationInWindow: CGPoint { point }
    override var modifierFlags: NSEvent.ModifierFlags { flags }
    override var scrollingDeltaX: CGFloat { delta.x }
    override var scrollingDeltaY: CGFloat { delta.y }
    override var hasPreciseScrollingDeltas: Bool { precise }
    override var magnification: CGFloat { magnificationDelta }
    override var rotation: Float { rotationDelta }
}

/// Standalone tablet values exercise the driver path that has no mouseDown.
private final class TabletEvent: NSEvent {
    var point = CGPoint.zero
    var force: Float = 1
    var time = ProcessInfo.processInfo.systemUptime
    var nativeType: NSEvent.EventType = .tabletPoint
    var nativeSubtype: NSEvent.EventSubtype = .tabletPoint
    var nativeButton = 0
    var device = 1
    var angle: Float = 0
    var capabilities = 0
    var entering = true
    var pointerType: NSEvent.PointingDeviceType = .pen
    var lean = CGPoint.zero
    var vendor: Int = 0
    var serial: Int = 0
    var unique: UInt64 = 0
    override var type: NSEvent.EventType { nativeType }
    override var buttonNumber: Int { nativeButton }
    override var subtype: NSEvent.EventSubtype { nativeSubtype }
    override var locationInWindow: CGPoint { point }
    override var modifierFlags: NSEvent.ModifierFlags { [] }
    override var pressure: Float { force }
    override var timestamp: TimeInterval { time }
    override var deviceID: Int { device }
    override var buttonMask: NSEvent.ButtonMask { force > 0 ? .penTip : [] }
    override var tilt: NSPoint { lean }
    override var rotation: Float { angle }
    override var capabilityMask: Int { capabilities }
    override var isEnteringProximity: Bool { entering }
    override var pointingDeviceType: NSEvent.PointingDeviceType { pointerType }
    override var vendorID: Int { vendor }
    override var pointingDeviceSerialNumber: Int { serial }
    override var uniqueID: UInt64 { unique }
}

private struct TabletRecordExpectation {
    let name: String
    let timestamp: UInt64
    let phase: String
    let flags: UInt32
    let rotation: Double
    let contact: String?
}

@MainActor private final class PenRestoreProjection {
    let ui = EditorSnapshotState()
    var failure: String?
    var publicationKeys: [[String]] = []
    func receive(_ next: JSON?, _ error: String?) {
        if let error { failure = error }
        guard let next else { return }
        publicationKeys.append(next.object.keys.sorted().prefix(64).map { String($0.prefix(128)) })
        if publicationKeys.count > 16 { publicationKeys.removeFirst() }
        ui.receive(next)
    }
}

private final class PaperCaptureGate: @unchecked Sendable {
    private let semaphore = DispatchSemaphore(value:0)
    private let lock = NSLock()
    private var started = false
    private var finished = false
    private var expired = false
    func hold(_ accepted: Bool) {
        lock.lock(); started = accepted; lock.unlock()
        let timeout = semaphore.wait(timeout:.now() + 10) == .timedOut
        lock.lock(); expired = timeout; finished = true; lock.unlock()
    }
    func release() { semaphore.signal() }
    var entered: Bool { lock.lock(); defer { lock.unlock() }; return started }
    var completed: Bool { lock.lock(); defer { lock.unlock() }; return finished }
    var timedOut: Bool { lock.lock(); defer { lock.unlock() }; return expired }
}

/// The assembled editor, AppKit event queue, serial owner and Metal renderer.
/// Actions choose tools; pointer/key/focus events use the native window through
/// the visible editor's actual hit targets. Not physical Pencil/OS menu delivery.
@main final class CanvasNativeInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func paperAttribute(_ root: NSObject, _ name: String) -> Any? {
        let selector = NSSelectorFromString(name)
        return root.responds(to:selector) ? root.perform(selector)?.takeUnretainedValue() : nil
    }
    @MainActor static func paperElement(_ root: NSObject, _ id: String) -> NSObject? {
        if paperAttribute(root,"accessibilityIdentifier") as? String == id { return root }
        return (paperAttribute(root,"accessibilityChildren") as? [NSObject] ?? []).lazy.compactMap { paperElement($0,id) }.first
    }
    @MainActor static func paperButton(_ root: NSObject, label: String) -> NSObject? {
        if paperAttribute(root,"accessibilityRole") as? String == "AXButton",
            paperAttribute(root,"accessibilityLabel") as? String == label { return root }
        return (paperAttribute(root,"accessibilityChildren") as? [NSObject] ?? []).lazy.compactMap { paperButton($0,label:label) }.first
    }
    @MainActor static func paperPress(_ root: NSObject) -> Bool {
        let selector = NSSelectorFromString("accessibilityPerformPress")
        guard root.responds(to:selector) else { return false }
        let call = unsafeBitCast(root.method(for:selector),to:(@convention(c) (AnyObject,Selector) -> Bool).self)
        return call(root,selector)
    }
    @MainActor static func paperBounds(_ root: NSObject) -> CGRect {
        let selector = NSSelectorFromString("accessibilityFrame")
        guard root.responds(to:selector) else { return .zero }
        let call = unsafeBitCast(root.method(for:selector),to:(@convention(c) (AnyObject,Selector) -> CGRect).self)
        return call(root,selector)
    }
    @MainActor static func paperFields(_ root: NSView) -> [NSTextField] {
        (root as? NSTextField).map { [$0] } ?? root.subviews.flatMap(paperFields)
    }

    static func fileIO<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { done in
            NativeProjectTask.io.async { done.resume(with: Result(catching: work)) }
        }
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        let paperOnly = CommandLine.arguments.contains("--paper")
        let penResponseOnly = CommandLine.arguments.contains("--pen-response")
        let deviceFlagsOnly = CommandLine.arguments.contains("--device-flags")
        let retainArtifacts = paperOnly || penResponseOnly
        for (azimuth, x, y) in [(0.0, 1.0, 0.0), (Double.pi / 2, 0, 1), (Double.pi, -1, 0), (-Double.pi / 2, 0, -1)] {
            let tilt = StylusTilt.towardBarrel(altitude: .pi / 4, azimuth: azimuth)
            try require(abs(tilt.x - x * .pi / 4) < 1e-9 && abs(tilt.y - y * .pi / 4) < 1e-9,
                "Pencil azimuth \(azimuth) must tilt toward the barrel in view axes")
            let tablet = StylusTilt.towardBarrel(appKit: CGPoint(x: x / 2, y: -y / 2))
            try require(abs(tablet.x - x * .pi / 4) < 1e-9 && abs(tablet.y - y * .pi / 4) < 1e-9,
                "AppKit tilt, whose y axis points up, must tilt toward the barrel in view axes")
        }
        let base = retainArtifacts ? URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_CANVAS_CAPTURES"] ?? FileManager.default.temporaryDirectory.path)
            : FileManager.default.temporaryDirectory
        let root = base.appendingPathComponent("capy-canvas-input-\(UUID())")
        if retainArtifacts { try await fileIO { try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false) } }
        else { try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false) }
        defer { if !retainArtifacts { try? FileManager.default.removeItem(at: root) } }
        let png = root.appendingPathComponent("artwork.png")
        var project = root.appendingPathComponent("rulers.capy")
        let storageRoot = root.appendingPathComponent("paper-storage")
        let persistence = EditorPersistence(root:paperOnly || deviceFlagsOnly || penResponseOnly ? storageRoot : nil)
        let paperInput = root.appendingPathComponent("authored-paper.png")
        var cancelPaperImport = false
        if paperOnly {
            try await fileIO {
                var bytes = Data(count:4096)
                for index in 0..<4096 {
                    let x = index % 64, y = index / 64
                    bytes[index] = UInt8((x * 3 + y * 5) % 256)
                }
                guard let provider = CGDataProvider(data:bytes as CFData),
                    let image = CGImage(width:64,height:64,bitsPerComponent:8,bitsPerPixel:8,bytesPerRow:64,
                        space:CGColorSpaceCreateDeviceGray(),bitmapInfo:CGBitmapInfo(rawValue:CGImageAlphaInfo.none.rawValue),
                        provider:provider,decode:nil,shouldInterpolate:false,intent:.defaultIntent),
                    let destination = CGImageDestinationCreateWithURL(paperInput as CFURL,"public.png" as CFString,1,nil)
                    else { throw HostFailure(message:"Private authored grayscale fixture") }
                CGImageDestinationAddImage(destination,image,nil)
                try require(CGImageDestinationFinalize(destination),"Write private authored Paper PNG")
                try bytes.write(to:root.appendingPathComponent("authored-paper-gray.raw"),options:.atomic)
            }
        }
        let store = EditorStore(platform:platform,persistence:persistence)
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(
            open: { _, done in done(paperOnly && !cancelPaperImport ? [paperInput] : []) }, save: { _, type, done in done(type == .capyProject ? project : png) },
            create: { _, done in done(newSheet([128, 128], space: "Srgb", depth: "U8")) }, exportOptions: { $0.choose($0.recipe) }))
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
        var deliveredKeys: [String] = []
        let monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp,
            .otherMouseDown, .otherMouseDragged, .otherMouseUp, .keyDown, .keyUp]) { event in
            if event.window === window {
                if event.type == .keyDown || event.type == .keyUp { deliveredKeys.append("\(event.type.rawValue):\(event.isARepeat):\(event.charactersIgnoringModifiers ?? "")") }
                else { deliveredEvent = event.eventNumber }
            }
            return event
        }
        defer { if let monitor { NSEvent.removeMonitor(monitor) } }
        func action(_ value: [String: Any]) async throws {
            try await store.apply(value)
            try await drain(0.02)
            try require(store.failure == nil, store.failure ?? "")
        }
        func invoke(_ command: String) async throws { try await action(["type":"invoke", "command":command]) }
        func tool(_ value: Any) async throws { try await action(["type":"layer", "action":["op":"tool", "tool":value]]) }
        func enabled(_ command: String) -> Bool {
            store.state["commands"].array.first { $0["id"].string == command }?["enabled"].bool == true
        }
        func newDocument() async throws {
            let epoch = store.state["document_file"]["epoch"].uint
            var phase = "invoke"
            do {
                try await invoke("new_document")
                phase = "confirmation"
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
                phase = "document completion"
                try await wait("New 128px document", failure:{store.projectFiles.error ?? store.failure}) {
                    let document = store.state["tabs"].array.first
                    return store.state["document_file"]["epoch"].uint != epoch
                        && !store.projectFiles.creating && !store.projectFiles.busy && !store.projectFiles.confirming
                        && store.state["new_drawing"].isNull && store.state["requests"].array.isEmpty
                        && document?["width"].uint == 128 && document?["height"].uint == 128
                }
                phase = "native sheet dismissal"
                try await wait("Native confirmation dismissed") { window.attachedSheet == nil && window.isKeyWindow }
                try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
                phase = "fit canvas"
                try await invoke("fit_canvas")
                phase = "color"
                try await action(["type":"set_color", "rgba":[0,0,1,1]])
            } catch {
                let document = store.state["tabs"].array.first ?? JSON()
                try? await navigationSave(JSON(["platform":platform,"phase":phase,
                    "failure":String(error.localizedDescription.prefix(4096)),"time_ns":FrameTrace.now(),
                    "epoch_before":epoch,"epoch_current":store.state["document_file"]["epoch"].uint,
                    "creating":store.projectFiles.creating,"busy":store.projectFiles.busy,"confirming":store.projectFiles.confirming,
                    "new_drawing_open":!store.state["new_drawing"].isNull,
                    "new_drawing_keys":store.state["new_drawing"].object.keys.sorted().prefix(64).map { String($0.prefix(128)) },
                    "requests":store.state["requests"].array.prefix(8).map { ["id":$0["id"].raw,
                        "type":$0["kind"]["type"].raw,"document":$0["kind"]["request"]["type"].raw] },
                    "width":document["width"].raw,"height":document["height"].raw,
                    "project_error":String((store.projectFiles.error ?? "").prefix(4096)),
                    "native_failure":String((store.failure ?? "").prefix(4096)),
                    "native_sheet":window.attachedSheet != nil,"window_key":window.isKeyWindow]),
                    to:root.appendingPathComponent("new-document-failure.json"))
                throw error
            }
        }
        func pixels() async throws -> Data {
            try await fileIO { try? FileManager.default.removeItem(at: png) }
            for attempt in 1... {
                do { try await invoke("export_document"); break } catch let failure as HostFailure
                    where failure.message == "Finish the canvas interaction first" && attempt < 100 {
                    store.failure = nil; try await drain(0.02)
                }
            }
            var committed = false
            try await wait("Committed PNG export", failure: { store.projectFiles.error ?? store.failure }, step: {
                committed = try await fileIO { FileManager.default.fileExists(atPath: png.path) }
            }) { committed && !store.projectFiles.busy }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            return try await fileIO {
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
            try await invoke("save_document_as")
            try await wait("Committed ruler project", failure: { store.projectFiles.error ?? store.failure }) {
                FileManager.default.fileExists(atPath: project.path) && !store.projectFiles.busy && store.state["requests"].array.isEmpty
            }
            try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            return try savedRulers(project)
        }
        func send(_ type: NSEvent.EventType, _ point: CGPoint, flags: NSEvent.ModifierFlags = []) async throws {
            if type == .leftMouseDown {
                try require(window.isKeyWindow, "The owned canvas must have native focus before a contact")
            }
            let camera = store.state["camera"], scale = window.backingScaleFactor
            let angle = camera["rotation"].number, zoom = camera["zoom"].number
            let x = point.x * (camera["flipped"][0].bool ? -zoom : zoom)
            let y = point.y * (camera["flipped"][1].bool ? -zoom : zoom)
            let surface = CGPoint(x: camera["translation"][0].number + cos(angle) * x - sin(angle) * y,
                y: camera["translation"][1].number + sin(angle) * x + cos(angle) * y)
            let local = CGPoint(x: surface.x / scale, y: surface.y / scale)
            try require(canvas.bounds.contains(local), "The document point \(point) must be inside the real canvas: local=\(local), bounds=\(canvas.bounds), camera=\(camera.stableKey), host=\(host.bounds), window=\(window.frame)")
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
        func paperPackage(_ name: String) async throws -> Data {
            let destination = root.appendingPathComponent(name + ".capy")
            try await fileIO {
                if FileManager.default.fileExists(atPath: destination.path) { throw HostFailure(message:"Private package destination already exists") }
            }
            project = destination
            try await invoke("save_document_as")
            var committed = false
            try await wait("Paper package \(name)", failure: { store.projectFiles.error ?? store.failure }, step: {
                committed = try await fileIO { FileManager.default.fileExists(atPath: destination.path) }
            }) { committed && !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            return try await fileIO {
                let bytes = try Data(contentsOf: destination)
                try require(bytes.count <= 16 * 1024 * 1024, "Bounded private Paper package")
                return bytes
            }
        }
        func paperJourney() async throws {
            func control(_ owner: EditorStore? = nil) -> JSON {
                (owner ?? store).state["layer_properties"]["controls"].array.first { !$0["paper"].isNull } ?? JSON()
            }
            func libraryAction(_ operation: String, _ owner: EditorStore? = nil) -> JSON {
                control(owner)["paper"]["tiles"].array.flatMap { $0["actions"].array }
                    .first { $0["action"]["op"].string == operation } ?? JSON()
            }
            func checkpoint() throws -> UInt64 {
                let tabs = store.snapshot["document_tabs"]
                guard let stamp = tabs["session_stamps"].array.first(where: { $0["id"].uint == tabs["selected"].uint }),
                    !stamp["stamp"]["checkpoint"].isNull else { throw HostFailure(message:"Missing actual session checkpoint") }
                return stamp["stamp"]["checkpoint"].uint
            }
            func idle(_ owner: EditorStore? = nil) async throws {
                let owner = owner ?? store
                await withCheckedContinuation { (done: CheckedContinuation<Void,Never>) in
                    owner.native!.submit(2,JSON(["type":"bootstrap"])) { _ in done.resume() }
                }
                try await wait("Paper worker completion", failure: { owner.failure }) {
                    !owner.state.isNull && !owner.papers.busy && owner.state["requests"].array.isEmpty
                }
            }
            func selectPaper(_ owner: EditorStore? = nil) async throws {
                let owner = owner ?? store
                let layers = owner.state["layers"].array
                try require(layers.count == 2, "Only the fixture's authored paint and paper layers")
                try await owner.apply(["type":"layer","action":["op":"select_row","id":layers.last!["id"].raw,"extend":false,"toggle":false]])
                try await wait("Shared Paper controls", failure: { owner.failure }) { !control(owner).isNull }
            }
            func pressID(_ id: String, in root: NSObject? = nil, containerID: String? = nil) async throws {
                let root = root ?? host
                func target() -> NSObject? {
                    if let containerID { return paperElement(root,containerID).flatMap { paperElement($0,id) } }
                    return paperElement(root,id)
                }
                try await wait("Native Paper control \(id)") { target() != nil }
                try require(paperPress(target()!), "Native Paper control accepts press: \(id)")
                try await drain()
            }
            func popupElement(_ id: String) -> NSObject? {
                NSApp.windows.lazy.compactMap { paperElement($0,id) }.first
            }
            func openPicker() async throws {
                if paperElement(host,"tool-drawer").flatMap({ paperElement($0,"layer-properties-disclosure") }) == nil {
                    guard let entry = store.snapshot["header"]["model"]["zones"].array.flatMap(\.array).first(where: {
                        $0["item"]["kind"].string == "tool" && $0["item"]["control"]["kind"].string == "panel"
                            && $0["item"]["control"]["panel"].string == "layers"
                    }) else { throw HostFailure(message:"Missing shared Layers header entry") }
                    try await action(["type":"customize","action":["type":"toggle_header_drawer","id":entry["id"].raw]])
                }
                try await wait("Mounted Layers drawer") {
                    paperElement(host,"tool-drawer").flatMap { paperElement($0,"layer-properties-disclosure") } != nil
                }
                if paperElement(host,"tool-drawer").flatMap({ paperElement($0,"property-texture") }) == nil {
                    try await pressID("layer-properties-disclosure",containerID:"tool-drawer")
                }
                try await pressID("property-texture",containerID:"tool-drawer")
                try await wait("Presented native Choose Texture dialog") {
                    popupElement("texture-dialog") != nil && store.papers.texture != nil
                }
            }
            func clickTile(_ index: Int) async throws {
                guard let tile = popupElement("texture-tile-\(index)") else { throw HostFailure(message:"Missing native Paper tile") }
                let frame = paperBounds(tile)
                try require(frame.width > 0 && frame.height > 0, "Measured native Paper tile")
                let screenPoint = CGPoint(x:frame.midX,y:frame.midY)
                guard let popup = NSApp.windows.first(where: { paperElement($0,"texture-tile-\(index)") != nil }),
                    let content = popup.contentView else { throw HostFailure(message:"Owned Choose Texture window") }
                let local = content.convert(popup.convertPoint(fromScreen:screenPoint),from:nil)
                for type: NSEvent.EventType in [.leftMouseDown,.leftMouseUp] {
                    nextEvent += 1
                    try event(type,at:local,marker:content,number:nextEvent)
                    try await drain(0.05)
                }
                guard let apply = popupElement("texture-apply") else { throw HostFailure(message:"Missing Use Texture") }
                try require(paperPress(apply), "Use Texture accepts press")
                try await wait("Use Texture closes Choose Texture") { popupElement("texture-dialog") == nil && store.papers.texture == nil }
                try require(paperElement(host,"layer-properties-disclosure") != nil && paperElement(host,"property-texture") != nil,
                    "Choose Texture retains the parent Layers drawer and properties")
            }
            func preview(_ lease: NativePaperPreviews, id: String) async throws -> Data {
                try await fileIO {
                    guard let image = lease.decode().images[id], image.bitsPerPixel == 8,
                        let data = image.dataProvider?.data else { throw HostFailure(message:"Immutable gray Paper preview") }
                    let bytes = data as Data
                    try require(bytes.count == image.width * image.height && bytes.count <= 1024 * 1024,
                        "Bounded exact shared preview bytes")
                    return bytes
                }
            }
            func renameField(_ text: String) async throws {
                try await wait("Native Paper rename field") {
                    guard let sheet = window.attachedSheet, let content = sheet.contentView else { return false }
                    return paperElement(sheet,"paper-library-name") != nil && paperFields(content).filter { $0.isEditable && $0.isEnabled }.count == 1
                }
                let sheet = window.attachedSheet!
                let field = paperFields(sheet.contentView!).first { $0.isEditable && $0.isEnabled }!
                try require(sheet.makeFirstResponder(field), "Native Paper rename focus")
                guard let editor = field.currentEditor() as? NSTextView else { throw HostFailure(message:"Native Paper field editor") }
                editor.selectAll(nil); editor.insertText(text,replacementRange:editor.selectedRange())
                try await drain()
            }
            try await wait("Private mounted recovery ready", failure: { store.failure }) { !store.recovery.restoring }
            guard let workspaces = store.workspaces else { throw HostFailure(message:"Private Paper workspace owner") }
            try await workspaces.started("Private Paper workspace startup")
            guard let sketch = try await workspaces.workspaceRows().first(where:{ $0["title"].string == "Sketch" }) else {
                throw HostFailure(message:"Shared Sketch workspace")
            }
            try await workspaces.perform(["type":"switch","id":sketch["id"].string])
            guard let directory = persistence.locations?.papers else { throw HostFailure(message:"Private Paper library directory") }
            func paperTheme(_ theme: String) async throws {
                try await action(["type":"set_theme","theme":theme])
                window.appearance = NSAppearance(named:theme == "light" ? .aqua : .darkAqua)
                try await newDocument()
                try await invoke("pen"); try await action(["type":"select_brush","id":1])
                try await action(["type":"set_brush_size","value":13])
                let white = try await pixels()
                try await path([CGPoint(x:24,y:32),CGPoint(x:64,y:32),CGPoint(x:104,y:32)])
                let first = try await pixels()
                try await path([CGPoint(x:24,y:88),CGPoint(x:64,y:88),CGPoint(x:104,y:88)])
                let painted = try await pixels()
                try require(first != white && painted != first, "Two distinct supplied native pointer strokes")
                try await selectPaper(); try await idle()
                try require(!enabled("redo"),"Two-stroke fixture has no Redo entry")
                let before = try await paperPackage(theme + "-before-import"), beforeCheckpoint = try checkpoint()
                let importItem = control()["paper"]["import"]
                try require(!importItem.isNull, "Shared native library import is available")
                cancelPaperImport = true; store.papers.action(importItem); try await idle(); cancelPaperImport = false
                let chooserPackage = try await paperPackage(theme + "-chooser-cancel")
                try require(chooserPackage == before && checkpoint() == beforeCheckpoint,
                    "Injected chooser Cancel preserves exact authored package and checkpoint")
                store.papers.action(importItem)
                try await wait("Imported Paper model", failure: { store.failure }) { !libraryAction("rename_paper").isNull }
                try await idle()
                let imported = try await pixels()
                let importedPackage = try await paperPackage(theme + "-imported"), importedCheckpoint = try checkpoint()
                let libraryDirty = store.state["document_file"]["modified"].bool
                try require(!libraryDirty,"Committed private import package is clean")
                try require(imported == painted && importedCheckpoint != beforeCheckpoint && importedPackage != before, "Import changes authored Paper and checkpoint while preserving existing paint pixels")
                try await invoke("undo")
                let undoPixels = try await pixels(), undoPackage = try await paperPackage(theme + "-import-undo")
                try require(undoPixels == painted && undoPackage == before, "Import Undo restores exact pixels and package")
                try await invoke("redo")
                let redoPixels = try await pixels(), redoPackage = try await paperPackage(theme + "-import-redo")
                try require(redoPixels == imported && redoPackage == importedPackage, "Import Redo restores exact pixels and package")
                let id = libraryAction("rename_paper")["action"]["id"].string
                try require(!id.isEmpty, "Shared library identity")
                let lease: NativePaperPreviews? = await withCheckedContinuation { done in
                    store.native!.paperPreviews(ids:[id]) { done.resume(returning:$0) }
                }
                guard let lease else { throw HostFailure(message:"Native immutable preview lease") }
                let previewIndex = Int(control()["value"]["value"].uint)
                try require(control()["paper"]["tiles"][previewIndex]["id"].string == id, "The current tile names its library identity")
                let originalPreview = try await preview(lease,id:id)
                let diskPreview = try await fileIO {
                    let file = try FileHandle(forReadingFrom:directory.appendingPathComponent(id + ".preview"))
                    defer { try? file.close() }
                    let bytes = try file.read(upToCount:1024 * 1024 + 1) ?? Data()
                    try require(bytes.count <= 1024 * 1024,"Bounded private shared preview file")
                    return bytes
                }
                try require(originalPreview == diskPreview, "Transport preserves exact shared preview bytes")
                try await openPicker()
                try await clickTile(previewIndex)
                let sameTilePixels = try await pixels()
                try require(sameTilePixels == imported && checkpoint() == importedCheckpoint, "Current native tile does not author another edit")
                let peer = EditorStore(platform:platform,persistence:EditorPersistence(root:storageRoot),managedWorkspaces:false)
                let unrelated = EditorStore(platform:platform,persistence:EditorPersistence(root:root.appendingPathComponent("unrelated-" + theme)),managedWorkspaces:false)
                try await wait("Private peer states",failure:{ peer.failure ?? unrelated.failure }) { !peer.state.isNull && !unrelated.state.isNull }
                try await idle(peer); try await idle(unrelated); try await selectPaper(peer); try await selectPaper(unrelated)
                try require(libraryAction("rename_paper",peer)["action"]["id"].string == id, "Same-root peer loads the actual library")
                let otherGeneration = unrelated.papers.generation, unrelatedControl = control(unrelated)
                let otherControl = try await fileIO { unrelatedControl.stableKey }
                let peerGeneration = peer.papers.generation
                let (profilePreferences, profileMetadata, profileDirectory) = try await fileIO {
                    guard let locations = StorageLocations.within(root.appendingPathComponent("profile-locale-" + theme)) else {
                        throw HostFailure(message:"Private profile locale storage")
                    }
                    let preferences = ColorPreferencesStore(locations:locations)
                    guard let bytes = CGColorSpace(name:CGColorSpace.sRGB)?.copyICCData() as Data? else {
                        throw HostFailure(message:"Private ICC fixture bytes")
                    }
                    _ = try preferences.importProfile(bytes)
                    try Data("Broken private ICC".utf8).write(to:locations.colorProfiles.appendingPathComponent(String(repeating:"f",count:64) + ".icc"))
                    let metadata = try preferences.profiles()
                    try require(metadata.count == 2 && metadata.allSatisfy { $0["profile"].isNull }, "Two byte-free retained profile descriptors")
                    return (preferences,metadata,locations.colorProfiles)
                }
                let profileWindow = NSWindow(contentRect:CGRect(x:100,y:100,width:600,height:650),
                    styleMask:[.titled,.closable],backing:.buffered,defer:false)
                profileWindow.isReleasedWhenClosed = false
                profileWindow.contentView = NSHostingView(rootView:RetainedProfileLibraryFixture(store:store,preferences:profilePreferences))
                profileWindow.orderFront(nil)
                defer { profileWindow.contentView = nil; profileWindow.close() }
                func profileRowsVisible(_ tag:String) -> Bool {
                    func labels(_ element:NSObject) -> [String] {
                        let label = paperAttribute(element,"accessibilityLabel") as? String ?? ""
                        let text = label.isEmpty ? paperAttribute(element,"accessibilityValue") as? String ?? "" : label
                        let own = paperAttribute(element,"accessibilityRole") as? String == "AXStaticText" ? [text] : []
                        return own + (paperAttribute(element,"accessibilityChildren") as? [NSObject] ?? []).flatMap(labels)
                    }
                    let actual = Set(labels(profileWindow))
                    let projected = ColorUI.profileEntries(profileMetadata,language:tag)
                    guard profileMetadata.count == 2 && projected.count == profileMetadata.count else { return false }
                    return projected.allSatisfy { row in
                        actual.contains(row["name"].string) && actual.contains(row[row["issue"].isNull ? "details" : "issue"].string)
                    }
                }
                try await wait("Nonempty profile rows mounted in English") { profileRowsVisible("en") }
                try await fileIO {
                    for entry in profileMetadata {
                        try FileManager.default.removeItem(at:profileDirectory.appendingPathComponent(entry["id"].string + ".icc"))
                    }
                }
                store.papers.action(libraryAction("rename_paper"))
                try await renameField("Cotton draft")
                guard let renameSheet = window.attachedSheet, let renameDraft = store.papers.rename?.id else {
                    throw HostFailure(message:"Existing native Paper rename sheet and draft")
                }
                for (tag, title) in [("ja", "用紙の名前を変更"), ("en", "Rename Paper")] {
                    guard let index = store.bootstrap["shipped_tags"].array.firstIndex(where:{ $0.string == tag }) else {
                        throw HostFailure(message:"Registered Paper rename language")
                    }
                    try await action(["type":"preferences","action":["type":"edit","id":"language","value":index + 1]])
                    func hasHeading(_ element: NSObject) -> Bool {
                        let label = paperAttribute(element,"accessibilityLabel") as? String
                            ?? paperAttribute(element,"accessibilityValue") as? String
                        if paperAttribute(element,"accessibilityRole") as? String == "AXStaticText", label == title { return true }
                        return (paperAttribute(element,"accessibilityChildren") as? [NSObject] ?? []).contains(where:hasHeading)
                    }
                    try await wait("Paper rename language publishes " + tag) { store.interfaceLanguage == tag }
                    try await wait("Paper rename sheet heading follows " + tag) { hasHeading(renameSheet) }
                    try await wait("Retained profile rows follow " + tag + " without their ICC files") { profileRowsVisible(tag) }
                    try require(profileWindow.contentView != nil, "Language keeps the nonempty profile library mounted")
                    let field = paperFields(renameSheet.contentView!).first { $0.isEditable && $0.isEnabled }
                    let name = (field?.currentEditor() as? NSTextView)?.string ?? field?.stringValue
                    try require(window.attachedSheet === renameSheet && store.papers.rename?.id == renameDraft
                        && name == "Cotton draft" && checkpoint() == importedCheckpoint,
                        "Language preserves the native Paper rename sheet, draft, typed name and artwork checkpoint")
                }
                try await renameField("")
                try await pressID("paper-name-save",in:window.attachedSheet!)
                try await wait("Shared Paper rename validation") { window.attachedSheet.map { paperElement($0,"paper-name-error") != nil } == true }
                try require(checkpoint() == importedCheckpoint, "Invalid native rename does not edit artwork")
                try await renameField("Cotton rag")
                try await pressID("paper-name-save",in:window.attachedSheet!)
                try await wait("Native Paper rename commits",failure:{ store.failure }) {
                    window.attachedSheet == nil && libraryAction("rename_paper")["action"]["name"].string == "Cotton rag"
                }
                try await idle(); try await idle(peer)
                try require(peer.papers.generation > peerGeneration && libraryAction("rename_paper",peer)["action"]["name"].string == "Cotton rag",
                    "Mutable library reload reaches only stores sharing its actual directory")
                let afterRenameControl = control(unrelated)
                let afterRenameKey = try await fileIO { afterRenameControl.stableKey }
                try require(unrelated.papers.generation == otherGeneration && afterRenameKey == otherControl,
                    "Unrelated native library transport is not invalidated")
                try require(store.state["document_file"]["modified"].bool == libraryDirty && checkpoint() == importedCheckpoint,"Rename preserves authored dirty state and checkpoint before Save As")
                store.papers.action(libraryAction("delete_paper")); try await idle()
                try await wait("Deleted library entry remains authored Paper") { libraryAction("rename_paper").isNull && !libraryAction("add_paper").isNull }
                try require(store.state["document_file"]["modified"].bool == libraryDirty,"Delete preserves authored dirty state before Save As")
                try require(try await preview(lease,id:id) == originalPreview, "Retained immutable preview survives deletion")
                let deletedPixels = try await pixels(), deletedPackage = try await paperPackage(theme + "-deleted")
                try require(deletedPixels == imported && deletedPackage == importedPackage && checkpoint() == importedCheckpoint,
                    "Rename and Delete preserve exact authored Paper, pixels and checkpoint")
                guard let native = store.native else { throw HostFailure(message:"Mounted native owner") }
                let add = libraryAction("add_paper"), gate = PaperCaptureGate()
                do {
                    defer { gate.release() }
                    native.submit(0,JSON(["type":"effect","action":add["action"].raw])) { result in gate.hold(result != nil) }
                    try await wait("Published Add pending capture",failure:{ store.failure }) {
                        gate.entered && store.papers.busy && store.state["requests"].array.contains { $0["kind"]["request"]["type"].string == "add" }
                    }
                    guard let progress = paperElement(host,"paper-library-progress"),
                        let cancel = paperButton(progress,label:store.bootstrap["common"]["cancel"].string) else { throw HostFailure(message:"Mounted Paper Cancel button") }
                    try require(paperPress(cancel), "Native Cancel before queued capture")
                }
                try await idle(); try require(gate.completed && !gate.timedOut, "Finite owned queue gate released")
                store.papers.reload(); try await idle()
                try require(store.state["document_file"]["modified"].bool == libraryDirty,"Cancelled Add preserves dirty state before Save As")
                try require(libraryAction("rename_paper").isNull && !libraryAction("add_paper").isNull,
                    "Cancel before capture must not recreate the deleted library entry")
                let cancelledPixels = try await pixels(), cancelledPackage = try await paperPackage(theme + "-cancel-before-capture")
                try require(cancelledPixels == imported && cancelledPackage == importedPackage && checkpoint() == importedCheckpoint,
                    "Cancelled Add preserves exact artwork and history checkpoint")
                store.papers.action(libraryAction("add_paper"))
                try await wait("Fresh Add after cancellation",failure:{ store.failure }) { libraryAction("rename_paper")["action"]["id"].string == id }
                try await idle(); store.papers.reload(); try await idle(); try await idle(peer)
                try require(store.state["document_file"]["modified"].bool == libraryDirty,"Add/reload preserves authored dirty state before Save As")
                try require(libraryAction("rename_paper",peer)["action"]["id"].string == id && unrelated.papers.generation == otherGeneration,
                    "Add and fresh reload retire cancellation and retain library isolation")
                let addedPixels = try await pixels(), addedPackage = try await paperPackage(theme + "-added")
                try require(addedPixels == imported && addedPackage == importedPackage && checkpoint() == importedCheckpoint,
                    "Library Add and reload do not author history")
                let afterAddControl = control(unrelated)
                try require(try await fileIO { afterAddControl.stableKey } == otherControl,"Unrelated library model remains exact after Add")
                for expected in [painted,first,white] { try await invoke("undo"); try require(try await pixels() == expected,"Exact known three-edit Undo sequence") }
                try require(!enabled("undo"), "Library actions added no hidden Undo entry")
                for expected in [first,painted,imported] { try await invoke("redo"); try require(try await pixels() == expected,"Exact known three-edit Redo sequence") }
                try require(!enabled("redo"), "Known history exhausted")
                try await action(["type":"effect","action":["op":"reset","layer":store.state["layer_properties"]["layer"].raw,"key":"texture"]])
                try await idle()
                note("PASS mounted Mac Paper \(theme): supplied pointer, native picker/Rename/Cancel, exact authored bytes/history and scoped library transport; physical UIKit/presentation/performance UNVERIFIED")
            }
            for theme in ["light","dark"] { try await paperTheme(theme) }
            guard let seedIndex = control()["paper"]["tiles"].array.firstIndex(where: {
                $0["actions"].array.contains { $0["action"]["op"].string == "rename_paper" }
            }) else { throw HostFailure(message:"Existing shared-produced seed image") }
            try await action(["type":"effect","action":["op":"set","layer":store.state["layer_properties"]["layer"].raw,
                "key":"texture","value":["kind":"choice","value":seedIndex]]])
            try await idle()
            let seedID = libraryAction("rename_paper")["action"]["id"].string
            try require(!seedID.isEmpty && !seedID.contains("/") && !seedID.contains("."), "Shared seed identity is a single filename")
            store.papers.action(libraryAction("rename_paper"))
            try await renameField("Cold paper")
            try await pressID("paper-name-save",in:window.attachedSheet!)
            try await idle()
            try require(libraryAction("rename_paper")["action"]["name"].string == "Cold paper", "Shared seed rename committed")
            guard let seedNative = store.native, let seedSettings = persistence.locations?.settings else {
                throw HostFailure(message:"Persistent owned seed owner")
            }
            for seedTheme in ["light","dark"] {
                try await action(["type":"set_theme","theme":seedTheme]); try await idle()
                let saved = await withCheckedContinuation { done in seedNative.flushPersistence { done.resume(returning:$0) } }
                try require(saved,"Shared seed preferences persisted through the owner/write/ack barrier")
                let seedName = "capy-test-drawing-paper-" + seedTheme + "-" + UUID().uuidString
                let destination = root.appendingPathComponent(seedName)
                try await fileIO {
                    let manager = FileManager.default
                    func read(_ url:URL, limit:Int) throws -> Data {
                        let properties = try url.resourceValues(forKeys:[.isRegularFileKey,.isSymbolicLinkKey])
                        try require(properties.isRegularFile == true && properties.isSymbolicLink != true,"Owned regular seed file")
                        let file = try FileHandle(forReadingFrom:url); defer { try? file.close() }
                        let bytes = try file.read(upToCount:limit + 1) ?? Data()
                        try require(bytes.count <= limit,"Bounded shared-produced seed file")
                        return bytes
                    }
                    let settings = try read(seedSettings,limit:AtomicJSONFile.maximumBytes)
                    let decoded = try JSON.decode(String(decoding:settings,as:UTF8.self))
                    try require(decoded["new_document"]["paper"].string == seedID && decoded["theme"].string == seedTheme,
                        "Persisted shared settings retain the selected image and explicit theme")
                    let index = try read(directory.appendingPathComponent("index.json"),limit:1024*1024)
                    let decodedIndex = try JSON.decode(String(decoding:index,as:UTF8.self))
                    try require(decodedIndex["papers"].array.count == 1 && decodedIndex["papers"][0]["id"].string == seedID
                        && decodedIndex["papers"][0]["name"].string == "Cold paper","One actual shared-produced seed entry")
                    let png = try read(directory.appendingPathComponent(seedID + ".png"),limit:1024*1024)
                    let preview = try read(directory.appendingPathComponent(seedID + ".preview"),limit:96*96)
                    try require(preview.count == 96*96,"Exact shared preview extent")
                    let files:[(String,Data)] = [("config/settings.json",settings),("data/papers/index.json",index),
                        ("data/papers/" + seedID + ".png",png),("data/papers/" + seedID + ".preview",preview)]
                    try require(!manager.fileExists(atPath:destination.path),"Fresh UUID seed destination")
                    try manager.createDirectory(at:destination,withIntermediateDirectories:false)
                    var entries:[[String:Any]] = []
                    for (relative,bytes) in files {
                        let target = destination.appendingPathComponent(relative)
                        try manager.createDirectory(at:target.deletingLastPathComponent(),withIntermediateDirectories:true)
                        try bytes.write(to:target,options:.withoutOverwriting)
                        try require(try read(target,limit:bytes.count) == bytes,"Seed copy retains exact original bytes")
                        entries.append(["path":relative,"bytes":bytes.count,
                            "sha256":SHA256.hash(data:bytes).map { String(format:"%02x",$0) }.joined()])
                    }
                    try require(!manager.fileExists(atPath:destination.appendingPathComponent("state").path)
                        && !manager.fileExists(atPath:destination.appendingPathComponent("cache").path)
                        && !manager.fileExists(atPath:destination.appendingPathComponent("temp").path)
                        && !manager.fileExists(atPath:destination.appendingPathComponent("data/workspaces").path),
                        "Cold seed excludes recovery, sessions, workspace state and caches")
                    let report:[String:Any] = ["profile":seedName,"theme":seedTheme,"paperID":seedID,"label":"Cold paper",
                        "files":entries,"defaults":decoded["new_document"]["defaults"].raw,"recoveryCopied":false]
                    let reportBytes = try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys])
                    try require(reportBytes.count <= 16384,"Bounded seed manifest outside the profile")
                    try reportBytes.write(to:root.appendingPathComponent(seedName + "-manifest.json"),options:.withoutOverwriting)
                }
            }
            let closed = await withCheckedContinuation { done in store.prepareClose { done.resume(returning:$0) } }
            try require(closed, "Private Paper persistence/recovery close")
            try await fileIO {}; try await drain(2)
        }
        func checkpoint() throws -> UInt64 {
            let tabs = store.snapshot["document_tabs"]
            guard let stamp = tabs["session_stamps"].array.first(where: { $0["id"].uint == tabs["selected"].uint }),
                !stamp["stamp"]["checkpoint"].isNull else { throw HostFailure(message:"Missing actual session checkpoint") }
            return stamp["stamp"]["checkpoint"].uint
        }
        func penResponseJourney() async throws {
            guard let native = store.native, let theme = ProcessInfo.processInfo.environment["CAPY_PEN_THEME"],
                ["light","dark"].contains(theme) else { throw HostFailure(message:"Mounted pen response owner and explicit light/dark theme") }
            func press(_ id:String) async throws {
                try await wait("Native pen response control \(id)") { paperElement(host,id) != nil }
                try require(paperPress(paperElement(host,id)!), "Activate actual native pen response control \(id)")
                try await drain()
            }
            try await wait("Native response owner recovery ready",failure:{store.failure}) { !store.recovery.restoring }
            try await action(["type":"set_theme", "theme":theme]); try await newDocument()
            let baseline = try await pixels(), originalCheckpoint = try checkpoint()
            let history = [enabled("undo"),enabled("redo")], camera = store.state["camera"].stableKey
            try await invoke("pen_tilt")
            try await wait("Measured native Tilt utility") { store.penResponseBounds != nil && paperElement(host,"pen-tilt-apply") != nil }
            try require(!store.state["tilt_calibration"]["ready"].bool, "Cold mounted Tilt must await its first native tablet callback")
            let epoch = store.state["tilt_calibration"]["editor"]["controls"]["epoch"].uint
            let untouched = store.state["tilt_calibration"]["editor"]["points"].stableKey
            let tablet = TabletEvent(); tablet.device = 71; tablet.vendor = 9; tablet.unique = 123
            tablet.nativeType = .tabletProximity; tablet.nativeSubtype = .tabletProximity
            canvas.tabletProximity(with:tablet)
            let area = store.camera.value["work_area"].array.map(\.number), scale = Double(window.backingScaleFactor)
            try require(area.count == 4 && area.allSatisfy({ $0.isFinite }) && area[2] > 0 && area[3] > 0
                && scale.isFinite && scale > 0, "Finite positive published Mac canvas work area")
            let workArea = CGRect(x:area[0]/scale,y:area[1]/scale,width:area[2]/scale,height:area[3]/scale).intersection(canvas.bounds)
            try require(!workArea.isNull && [workArea.minX,workArea.minY,workArea.width,workArea.height].allSatisfy({ $0.isFinite })
                && workArea.width > 0 && workArea.height > 0, "Published work area intersects the native canvas")
            let insetX = min(24,workArea.width/2), insetY = min(24,workArea.height/2)
            let candidates = [CGPoint(x:workArea.minX+insetX,y:workArea.midY),
                CGPoint(x:workArea.midX,y:workArea.maxY-insetY),CGPoint(x:workArea.midX,y:workArea.midY)]
            guard let point = candidates.first(where: { point in
                store.penResponseBounds?.contains(point) != true
                    && host.hitTest(canvas.convert(point,to:host)) === canvas
            }) else { throw HostFailure(message:"Clear native canvas point outside modeless Tilt utility") }
            tablet.point = canvas.convert(point,to:nil); tablet.force = 0; tablet.lean = CGPoint(x:0.2,y:-0.3)
            tablet.nativeType = .tabletPoint; tablet.nativeSubtype = .tabletPoint
            canvas.tabletPoint(with:tablet)
            try await wait("First native tablet callback binds the mounted Tilt utility") { store.state["tilt_calibration"]["ready"].bool }
            try require(store.state["tilt_calibration"]["editor"]["controls"]["epoch"].uint == epoch
                && store.state["tilt_calibration"]["editor"]["points"].stableKey == untouched,
                "Native first binding retains the displayed curve owner and response")
            try await press("pen-tilt-lighter")
            try await wait("Actual sensitivity control edits shared response") { store.state["tilt_calibration"]["editor"]["points"].stableKey != untouched }
            try await press("pen-tilt-reverse-horizontal")
            try await wait("Actual independent axis control edits shared response") { store.state["tilt_calibration"]["axes"]["reverse_horizontal"].bool }
            try await press("pen-tilt-apply")
            try await wait("Actual native Apply closes Tilt") { store.state["tilt_calibration"].isNull }
            let saved = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning:$0) } }
            try require(saved && store.failure == nil, store.failure ?? "Native response Apply persistence")
            let source = NativePenDevice.appKitKey(vendor:9,unique:123,serial:0)
            let profile = store.state["settings"]["tilt_calibrations"][source].stableKey
            try require(!store.state["settings"]["tilt_calibrations"][source].isNull, "Actual tablet source key must own the applied response")
            let disk = try await fileIO { try JSON.decode(String(decoding:Data(contentsOf:persistence.locations!.settings),as:UTF8.self)) }
            try require(disk["tilt_calibrations"][source].stableKey == profile, "Native Apply flush stores exact source response")
            let restoredProjection = PenRestoreProjection()
            let restored = try NativeOwner(platform:platform,persistence:persistence,receive:{ next,error in
                DispatchQueue.main.async { MainActor.assumeIsolated { restoredProjection.receive(next,error) } }
            })
            await withCheckedContinuation { (done: CheckedContinuation<Void,Never>) in
                restored.submit(2,JSON(["type":"catalog"])) { _ in DispatchQueue.main.async { done.resume() } }
            }
            let restoredSettings = restoredProjection.ui.state["settings"]
            try await navigationSave(JSON(["platform":platform,"theme":theme,"source_key":source,
                "publication_keys":restoredProjection.publicationKeys,"has_settings":!restoredSettings.isNull,
                "native_failure":restoredProjection.failure.map { String($0.prefix(4096)) } as Any? ?? NSNull(),
                "profile":restoredSettings["tilt_calibrations"][source].raw,"expected_profile_key":profile]),
                to:root.appendingPathComponent("pen-restore-projection.json"))
            try require(restoredProjection.failure == nil && restoredSettings["tilt_calibrations"][source].stableKey == profile,
                "Fresh owner restores the response selected by actual native controls")
            let unchanged = try await pixels()
            try require(unchanged == baseline && checkpoint() == originalCheckpoint && [enabled("undo"),enabled("redo")] == history
                && store.state["camera"].stableKey == camera, "Native calibration controls preserve exact artwork, history and camera")
            tablet.nativeType = .tabletProximity; tablet.nativeSubtype = .tabletProximity; tablet.entering = false
            canvas.tabletProximity(with:tablet)
            try await navigationSave(JSON(["platform":platform,"theme":theme,"source_key":source,
                "delivery":"supplied public AppKit tablet callbacks and actual native accessibility presses",
                "profile":store.state["settings"]["tilt_calibrations"][source].raw,"physical_pen":false]),
                to:root.appendingPathComponent("pen-response-\(theme).json"))
            note("PASS AppKit policy \(platform) \(theme): first tablet binding, native sensitivity/axis/Apply, private durable source profile and exact artwork/history")
        }
        func tabletDeviceFlags() async throws {
            guard let native = store.native else { throw HostFailure(message: "Mounted native tablet owner") }
            let decoder = ProcessInfo.processInfo.environment["CAPY_STROKE_RECORDING_TOOL"]
                ?? FileManager.default.currentDirectoryPath + "/target/debug/examples/stroke-recording"
            func recording(_ action: String) async throws -> JSON {
                let reply: JSON = await withCheckedContinuation { done in
                    store.query(["type": "stroke_recording", "action": action]) { done.resume(returning: $0) }
                }
                try require(!reply.isNull && store.failure == nil, store.failure ?? "Stroke recording query \(action)")
                return reply
            }
            func flush() async throws {
                await withCheckedContinuation { done in store.query(["type": "catalog"]) { _ in done.resume() } }
                try require(store.failure == nil, store.failure ?? "Tablet input queue")
            }
            for theme in ["light", "dark"] {
                try await action(["type": "set_theme", "theme": theme]); try await newDocument()
                try await invoke("pen")
                try await wait("Prepared tablet brush") { store.snapshot["brush_ready"].bool }
                try require(try await recording("start")["recording"].bool, "Start native tablet recording")
                var expected: [TabletRecordExpectation] = []
                var ignored: [UInt64] = []
                var timestamp: UInt64 = 0
                func pose(_ event: TabletEvent, x: Double = 64) {
                    let camera = store.state["camera"], scale = window.backingScaleFactor
                    let local = CGPoint(x: (camera["translation"][0].number + x * camera["zoom"].number) / scale,
                        y: (camera["translation"][1].number + 64 * camera["zoom"].number) / scale)
                    event.point = canvas.convert(local, to: nil)
                    timestamp = max(timestamp + 1_000_000, UInt64(ProcessInfo.processInfo.systemUptime * 1_000_000_000))
                    event.time = Double(timestamp) / 1_000_000_000
                }
                func expect(_ name: String, _ event: TabletEvent, phase: String, flags: UInt32,
                    rotation: Double? = nil, contact: String? = nil) {
                    expected.append(TabletRecordExpectation(name: name, timestamp: UInt64(event.time * 1_000_000_000),
                        phase: phase, flags: flags, rotation: rotation ?? Double(event.angle) * .pi / 180, contact: contact))
                }
                func proximity(_ event: TabletEvent, mask: Int, entering: Bool = true,
                    terminal: String? = nil, flags: UInt32 = 0, contact: String? = nil) async throws {
                    pose(event)
                    event.nativeType = .tabletProximity; event.nativeSubtype = .tabletProximity
                    event.capabilities = mask; event.entering = entering
                    canvas.tabletProximity(with: event)
                    if let terminal { expect(terminal, event, phase: "Up", flags: flags, rotation: 0, contact: contact) }
                    try await flush()
                }
                func point(_ event: TabletEvent, _ name: String?, force: Float, phase: String,
                    flags: UInt32 = 0, contact: String? = nil) async throws {
                    pose(event)
                    event.nativeType = .tabletPoint; event.nativeSubtype = .tabletPoint; event.force = force
                    canvas.tabletPoint(with: event)
                    if let name { expect(name, event, phase: phase, flags: flags, contact: contact) }
                    else { ignored.append(UInt64(event.time * 1_000_000_000)) }
                    try await flush()
                }
                func mouse(_ event: TabletEvent, _ name: String, type: NSEvent.EventType,
                    flags: UInt32, contact: String) async throws {
                    pose(event)
                    event.nativeType = type; event.nativeSubtype = .tabletPoint
                    let phase: String
                    switch type {
                    case .leftMouseDown: phase = "Down"; canvas.mouseDown(with: event)
                    case .leftMouseDragged: phase = "Move"; canvas.mouseDragged(with: event)
                    default: phase = "Up"; canvas.mouseUp(with: event)
                    }
                    expect(name, event, phase: phase, flags: flags, contact: contact)
                    try await flush()
                }
                let barrel = UInt32(CAPY_APPLE_BARREL_TWIST)
                let first = TabletEvent(); first.device = 71; first.force = 0
                let second = TabletEvent(); second.device = 72; second.angle = 45; second.force = 0
                try await proximity(first, mask: 0x2000)
                try await point(first, "confirmed zero hover", force: 0, phase: "Hover", flags: barrel)
                try await proximity(second, mask: 0x1000)
                try await point(second, "orientation bit is not roll", force: 0, phase: "Hover")
                try await point(second, "unconfirmed nonzero down", force: 1, phase: "Down", contact: "second")
                try await point(second, "unconfirmed nonzero move", force: 1, phase: "Move", contact: "second")
                try await point(second, "unconfirmed terminal", force: 0, phase: "Up", contact: "second")
                try await point(first, "device pairing retains zero hover", force: 0, phase: "Hover", flags: barrel)
                try await point(first, "confirmed zero down", force: 1, phase: "Down", flags: barrel, contact: "first")
                try await proximity(first, mask: 0)
                try await proximity(second, mask: 0x2000, entering: false)
                try await point(second, nil, force: 1, phase: "Move")
                try await point(first, "captured flags survive cache change", force: 1, phase: "Move", flags: barrel, contact: "first")
                try await proximity(first, mask: 0, entering: false, terminal: "proximity terminal retains capture", flags: barrel, contact: "first")
                first.angle = 90
                try await point(first, "post-exit unknown down", force: 1, phase: "Down", contact: "after-exit")
                try await point(first, "post-exit unknown move", force: 1, phase: "Move", contact: "after-exit")
                try await point(first, "post-exit terminal", force: 0, phase: "Up", contact: "after-exit")
                try await point(first, "post-exit nonzero hover", force: 0, phase: "Hover")
                let mouseTablet = TabletEvent(); mouseTablet.device = 73
                try await proximity(mouseTablet, mask: 0x2000)
                try await mouse(mouseTablet, "mouse subtype zero down", type: .leftMouseDown, flags: barrel, contact: "mouse")
                try await proximity(mouseTablet, mask: 0)
                try await mouse(mouseTablet, "mouse capture survives cache change", type: .leftMouseDragged, flags: barrel, contact: "mouse")
                mouseTablet.angle = 33; mouseTablet.force = 0
                try await mouse(mouseTablet, "mouse terminal retains capture", type: .leftMouseUp, flags: barrel, contact: "mouse")
                try await point(mouseTablet, "later hover uses updated cache", force: 0, phase: "Hover")
                try await proximity(mouseTablet, mask: 0, entering: false)
                let stopped = try await recording("stop")
                try require(stopped["ready"].bool && !stopped["recording"].bool, "Stop native tablet recording")
                let raw: Data = try await withCheckedThrowingContinuation { done in
                    native.strokeRecordingData { done.resume(with: $0) }
                }
                let directory = root.appendingPathComponent("tablet-flags-\(platform)-\(theme)")
                let expectedData = try JSONSerialization.data(withJSONObject: ["samples": expected.map {
                    ["name": $0.name, "timestamp_ns": $0.timestamp, "phase": $0.phase,
                     "flags": $0.flags, "rotation": $0.rotation, "contact": $0.contact as Any? ?? NSNull()] as [String: Any]
                }, "ignored_timestamp_ns": ignored], options: [.sortedKeys])
                let records: Data = try await fileIO {
                    try require(FileManager.default.isExecutableFile(atPath: decoder),
                        "Build the existing layer-engine stroke-recording example or set CAPY_STROKE_RECORDING_TOOL")
                    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
                    try expectedData.write(to: directory.appendingPathComponent("expected.json"))
                    let recording = directory.appendingPathComponent("capture.capystrokes")
                    try Data().write(to: recording)
                    let file = try FileHandle(forWritingTo: recording)
                    let error = raw.withUnsafeBytes { buffer in
                        takeNativeText(capy_stroke_recording_write(buffer.bindMemory(to: UInt8.self).baseAddress, raw.count, file.fileDescriptor))
                    }
                    try file.close()
                    if let error { throw HostFailure(message: error) }
                    let output = directory.appendingPathComponent("records.jsonl"), errors = directory.appendingPathComponent("decoder.log")
                    try Data().write(to: output); try Data().write(to: errors)
                    let stdout = try FileHandle(forWritingTo: output), stderr = try FileHandle(forWritingTo: errors)
                    defer { try? stdout.close(); try? stderr.close() }
                    let process = Process(); process.executableURL = URL(fileURLWithPath: decoder)
                    process.arguments = ["dump", recording.path]; process.standardOutput = stdout; process.standardError = stderr
                    try process.run(); process.waitUntilExit()
                    try require(process.terminationStatus == 0, "Shared recording decoder: " + (try String(contentsOf: errors, encoding: .utf8)))
                    let data = try Data(contentsOf: output)
                    if let captures = ProcessInfo.processInfo.environment["CAPY_CANVAS_CAPTURES"] {
                        let copy = URL(fileURLWithPath: captures).appendingPathComponent(directory.lastPathComponent)
                        try FileManager.default.createDirectory(at: copy, withIntermediateDirectories: true)
                        for name in ["capture.capystrokes", "records.jsonl", "decoder.log", "expected.json"] {
                            try Data(contentsOf: directory.appendingPathComponent(name)).write(to: copy.appendingPathComponent(name))
                        }
                    }
                    return data
                }
                let rawEvents = try String(decoding: records, as: UTF8.self).split(separator: "\n").map { try JSON.decode(String($0)) }
                    .map { $0["Raw"]["event"] }.filter { !$0.isNull }
                var contacts: [String: UInt64] = [:]
                for sample in expected {
                    let matches = rawEvents.filter { $0["timestamp_ns"].uint == sample.timestamp }
                    try require(matches.count == 1, "Exactly one native raw event for \(sample.name): \(matches.count)")
                    let event = matches[0]
                    try require(event["phase"].string == sample.phase && event["tool"].string == "Pen",
                        "Native phase/tool for \(sample.name): \(event.stableKey)")
                    try require(event["flags"].uint & 0xc0 == UInt64(sample.flags), "Captured device flags for \(sample.name): \(event.stableKey)")
                    try require(abs(event["twist_radians"].number - sample.rotation) < 1e-6,
                        "Native numeric rotation for \(sample.name): \(event.stableKey)")
                    if let contact = sample.contact {
                        if let id = contacts[contact] { try require(event["device_id"].uint == id, "Native contact identity for \(sample.name)") }
                        else { contacts[contact] = event["device_id"].uint }
                    }
                }
                try require(ignored.allSatisfy { timestamp in !rawEvents.contains { $0["timestamp_ns"].uint == timestamp } },
                    "An unrelated tablet device cannot inject into the captured contact")
                try require(Set(contacts.values).count == contacts.count, "Fresh tablet contacts keep distinct captured identities")
                _ = try await recording("saved")
                note("PASS AppKit source callbacks policy \(platform) \(theme): zero roll/capability, device pairing, frozen flags, hover and terminal; physical tablet UNVERIFIED")
            }
        }
        if penResponseOnly { try await penResponseJourney(); return }
        if deviceFlagsOnly {
            try await tabletDeviceFlags()
            let closed = await withCheckedContinuation { done in store.prepareClose { done.resume(returning: $0) } }
            try require(closed, "Close private device-capability storage")
            return
        }
        if paperOnly { try await paperJourney(); return }

        for theme in ["light", "dark"] {
            try await action(["type":"set_theme", "theme":theme]); try await newDocument()
            try await action(["type":"select_brush", "id":1]); try await action(["type":"set_brush_size", "value":13])
            try require(window.makeFirstResponder(canvas) && window.firstResponder === canvas, "Size keys belong to the mounted canvas")
            let paper = try await pixels(), camera = store.state["camera"].stableKey
            let undo = enabled("undo"), redo = enabled("redo"), start = deliveredKeys.count
            var expectedEvents: [String] = []
            for (characters, code, sizes) in [("]", UInt16(30), [15.0, 17, 20]), ("[", UInt16(33), [17.0, 15, 12, 10]), ("]", UInt16(30), [12.0]), ("]", UInt16(30), [15.0])] {
                let keyName = characters
                for (index, size) in sizes.enumerated() {
                    try key(keyName, code: code, window: window, pressed: true, repeatKey: index > 0)
                    expectedEvents.append("\(NSEvent.EventType.keyDown.rawValue):\(index > 0):\(keyName)")
                    try await wait("Size preset \(size) after \(keyName) repeat=\(index > 0)") { store.state["brush"]["diameter"].number == size }
                }
                try key(keyName, code: code, window: window, pressed: false)
                expectedEvents.append("\(NSEvent.EventType.keyUp.rawValue):false:\(keyName)")
                try await wait("Native size-key release") { Array(deliveredKeys.dropFirst(start)) == expectedEvents }
                try await drain(0.15)
                try require(store.state["brush"]["diameter"].number == sizes.last!, "Key release adds no extra size step")
            }
            try require(store.state["camera"].stableKey == camera && window.frame == originalWindow, "Size keys preserve camera and owned window")
            try require(enabled("undo") == undo && enabled("redo") == redo, "Size keys preserve artwork history")
            let unchanged = try await pixels(); try require(unchanged == paper, "Size keys preserve every artwork pixel")
            note("PASS platform \(platform) \(theme): mounted size key down/repeats/up, fresh same-key contact, unchanged camera/artwork/history")
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
        for (button, trigger) in [(1, "pen.button.primary"), (2, "pen.button.secondary")] where platform == 1 {
            try await invoke("settings")
            for step: [String: Any] in [["type": "edit_pen_button", "trigger": trigger],
                ["type": "open_pen_button_picker", "trigger": trigger, "category": NSNull()],
                ["type": "choose_action", "id": "command.Hand"]] {
                try await action(["type": "preferences", "action": step])
            }
            try await action(["type": "close_settings"])
            window.setFrame(originalWindow, display: true)
            let before = store.state["layer_tools"]["tool"].string
            let tablet = TabletEvent()
            tablet.nativeButton = button; tablet.force = 0
            tablet.point = canvas.convert(CGPoint(x: canvas.bounds.midX, y: canvas.bounds.midY), to: nil)
            tablet.nativeType = button == 1 ? .rightMouseDown : .otherMouseDown
            if button == 1 { canvas.rightMouseDown(with: tablet) } else { canvas.otherMouseDown(with: tablet) }
            try await wait("Held \(trigger)") { store.navigationCursor == "pan" }
            tablet.nativeType = button == 1 ? .rightMouseUp : .otherMouseUp
            if button == 1 { canvas.rightMouseUp(with: tablet) } else { canvas.otherMouseUp(with: tablet) }
            try await wait("Released \(trigger)") { store.navigationCursor != "pan" }
            try require(store.state["layer_tools"]["tool"].string == before, "Panning with \(trigger) keeps the tool")
            try await invoke("settings")
            try await action(["type": "preferences", "action": ["type": "reset_trigger", "trigger": trigger]])
            try await action(["type": "close_settings"])
            window.setFrame(originalWindow, display: true)
        }
        func moveOverCanvas() throws {
            let point = canvas.convert(CGPoint(x: canvas.bounds.midX, y: canvas.bounds.midY), to: nil)
            guard let event = NSEvent.mouseEvent(with: .mouseMoved, location: point, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                eventNumber: 0, clickCount: 0, pressure: 0) else { throw HostFailure(message: "No mouse event") }
            canvas.mouseMoved(with: event)
        }
        try moveOverCanvas()
        try require(NSCursor.current.image.size == NSSize(width: 1, height: 1), "The system arrow hides over the canvas")
        let painting = store.state["commands"].array.first {
            $0["selected"].bool && ["pen", "pencil", "brush", "drawing_brush", "airbrush"].contains($0["id"].string)
        }?["id"].string
        try await invoke("hand")
        try moveOverCanvas()
        try require(NSCursor.current == NSCursor.openHand, "The Hand tool shows the open hand over the canvas")
        try await invoke(painting ?? "pen")
        try moveOverCanvas()
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
            func recalledCamera(_ before: JSON) -> Bool {
                near(store.state["camera"]["zoom"].number, before["zoom"].number)
                    && near(store.state["camera"]["rotation"].number, before["rotation"].number)
                    && store.state["camera"]["flipped"].stableKey == before["flipped"].stableKey
                    && (0..<2).allSatisfy { near(store.state["camera"]["translation"][$0].number, before["translation"][$0].number) }
            }
            func gesture(_ rotate: Bool, phase: NSEvent.Phase, delta: Double = 0) async throws {
                event.eventPhase = phase
                event.magnificationDelta = rotate ? 0 : CGFloat(delta)
                event.rotationDelta = rotate ? Float(delta) : 0
                if rotate { canvas.rotate(with: event) } else { canvas.magnify(with: event) }
                try await flush()
            }
            func recall(_ before: JSON, completed: JSON) async throws {
                try await invoke("previous_view"); try await flush()
                try require(recalledCamera(before), "Previous View must recall the beginning of the entire native gesture")
                try await invoke("previous_view"); try await flush()
                try require(recalledCamera(completed), "Previous View must return to the completed native gesture")
            }
            func middle(_ type: NSEvent.EventType, at point: CGPoint) async throws {
                nextEvent += 1
                guard let mouse = NSEvent.mouseEvent(with: type, location: canvas.convert(point, to: nil),
                    modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, eventNumber: nextEvent, clickCount: 1, pressure: 1), let cg = mouse.cgEvent else {
                    throw HostFailure(message: "No middle-button event")
                }
                cg.setIntegerValueField(.mouseEventButtonNumber, value: 2)
                guard let delivery = NSEvent(cgEvent: cg) else { throw HostFailure(message: "Invalid middle-button event") }
                try require(delivery.buttonNumber == 2 && delivery.type == type, "Deliver the actual middle-button contact")
                NSApp.postEvent(delivery, atStart: false)
                try await wait("Native middle-button event delivery") { deliveredEvent == nextEvent }
                try await flush()
            }
            for cancelled in [true, false] {
                try await invoke("fit_canvas")
                var point = local
                try await middle(.otherMouseDown, at: point)
                var before = store.state["camera"]
                event.flags = .control; event.delta = CGPoint(x: 0, y: 40)
                canvas.scrollWheel(with: event); try await flush()
                try require(store.state["camera"]["zoom"].number > before["zoom"].number,
                    "A held middle button must admit Control-scroll zoom")
                before = store.state["camera"]
                event.flags = []; event.delta = CGPoint(x: 3, y: 4)
                canvas.scrollWheel(with: event); try await flush()
                try require(near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 3 * scale)
                    && near(store.state["camera"]["translation"][1].number - before["translation"][1].number, 4 * scale),
                    "A held middle button must admit normal scroll")
                before = store.state["camera"]
                try await middle(.otherMouseDragged, at: point)
                try require(sameCamera(before), "Stationary middle-button motion must preserve the scrolled camera")
                point.x += 5; point.y += 7
                try await middle(.otherMouseDragged, at: point)
                try require(near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 5 * scale)
                    && near(store.state["camera"]["translation"][1].number - before["translation"][1].number, 7 * scale),
                    "Middle-button dragging must continue from the scrolled camera")
                if cancelled { store.input(["type":"blur"]) }
                else { try await middle(.otherMouseUp, at: point) }
                try await flush()
                before = store.state["camera"]
                point.x += 5
                try await middle(.otherMouseDragged, at: point)
                try require(sameCamera(before), "Released or interrupted middle-button contacts must stop dragging")
                canvas.scrollWheel(with: event); try await flush()
                try require(!sameCamera(before), "Scroll must resume after middle-button release or interruption")
            }
            try await invoke("fit_canvas")
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
            event.eventPhase = []
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
            for theme in ["light", "dark"] {
                try await action(["type": "set_theme", "theme": theme])
                try await action(["type": "preferences", "action": ["type": "edit", "id": "wheel_behavior", "value": 1]])
                before = store.state["camera"]
                event.flags = []; event.delta = CGPoint(x: 0, y: 40)
                canvas.scrollWheel(with: event); try await flush()
                let ratio = store.state["camera"]["zoom"].number / before["zoom"].number
                try require(ratio > 1 && (0..<2).allSatisfy {
                    near(store.state["camera"]["translation"][$0].number,
                        [anchor.x, anchor.y][$0] + (before["translation"][$0].number - [anchor.x, anchor.y][$0]) * ratio)
                }, "Wheel Zoom must preserve its native anchor in \(theme)")
                before = store.state["camera"]
                event.flags = .control; event.delta = CGPoint(x: 0, y: 40)
                canvas.scrollWheel(with: event); try await flush()
                try require(store.state["camera"]["zoom"].number > before["zoom"].number,
                    "Control retains Wheel Zoom in \(theme)")
                before = store.state["camera"]
                event.flags = .shift; event.delta = CGPoint(x: 3, y: 4)
                canvas.scrollWheel(with: event); try await flush()
                try require(near(store.state["camera"]["zoom"].number, before["zoom"].number)
                    && near(store.state["camera"]["translation"][0].number - before["translation"][0].number, 7 * scale)
                    && near(store.state["camera"]["translation"][1].number, before["translation"][1].number),
                    "Shift pans horizontally with Wheel Zoom in \(theme)")
                try await action(["type": "preferences", "action": ["type": "edit", "id": "wheel_behavior", "value": 0]])
                try await action(["type": "preferences", "action": ["type": "edit", "id": "rotate_with_two_fingers", "value": false]])
                before = store.state["camera"]
                try await gesture(true, phase: .began); try await gesture(true, phase: .changed, delta: 20)
                try await gesture(true, phase: .ended)
                try require(sameCamera(before), "Disabled two-finger rotation must preserve the entire camera in \(theme)")
                try await gesture(false, phase: .began); try await gesture(false, phase: .changed, delta: 0.1)
                try await gesture(false, phase: .ended)
                try require(store.state["camera"]["zoom"].number > before["zoom"].number
                    && near(store.state["camera"]["rotation"].number, before["rotation"].number),
                    "Disabling rotation must preserve native pinch zoom in \(theme)")
                try await action(["type": "preferences", "action": ["type": "edit", "id": "rotate_with_two_fingers", "value": true]])
                for mode in ["pinch", "rotation", "combined"] {
                    let beginning = store.state["camera"]
                    if mode != "rotation" {
                        try await gesture(false, phase: .began)
                        try await gesture(false, phase: .changed, delta: 0.25)
                    }
                    if mode != "pinch" {
                        try await gesture(true, phase: .began)
                        try await gesture(true, phase: .changed, delta: 20)
                    }
                    if mode != "rotation" {
                        try await gesture(false, phase: .changed, delta: 0.10)
                        try await gesture(false, phase: .ended)
                    }
                    if mode != "pinch" {
                        try await gesture(true, phase: .changed, delta: 10)
                        try await gesture(true, phase: .ended)
                    }
                    let completed = store.state["camera"]
                    try require(!recalledCamera(beginning), "The supplied \(mode) gesture must move the view")
                    try await recall(beginning, completed: completed)
                }
                for interruption in ["cancel", "blur"] {
                    try await gesture(false, phase: .began)
                    try await gesture(false, phase: .changed, delta: 0.10)
                    if interruption == "cancel" { try await gesture(false, phase: .cancelled) }
                    else { store.input(["type": "blur"]); try await flush() }
                    let resumed = store.state["camera"]
                    try await gesture(true, phase: .began)
                    try await gesture(true, phase: .changed, delta: 10)
                    try await gesture(true, phase: .changed, delta: 5)
                    try await gesture(true, phase: .ended)
                    try await recall(resumed, completed: store.state["camera"])
                }
                try await tool("select")
                try await invoke("fit_canvas"); try await invoke("zoom_out")
                let earlierRecall = store.state["camera"]
                try await invoke("fit_canvas")
                let rejectedBegin = store.state["camera"]
                try require(!recalledCamera(earlierRecall), "The old recall view must differ from the first accepted gesture view")
                try await send(.leftMouseDown, CGPoint(x: 64, y: 64))
                try await gesture(false, phase: .began)
                try await gesture(true, phase: .began)
                try require(sameCamera(rejectedBegin), "Rejected gesture beginnings must preserve the captured contact view")
                try await send(.leftMouseUp, CGPoint(x: 64, y: 64)); try await flush()
                try require(try await pixels() == painted, "Releasing the stationary selection contact must preserve every pixel")
                let firstAccepted = store.state["camera"]
                try require(recalledCamera(rejectedBegin), "The released contact must leave the gesture's starting view unchanged")
                try await gesture(false, phase: .changed, delta: 0.10)
                try await gesture(true, phase: .changed, delta: 10)
                try await gesture(false, phase: .ended)
                try await gesture(true, phase: .changed, delta: 5)
                try await gesture(true, phase: .ended)
                try await recall(firstAccepted, completed: store.state["camera"])
                try require(try await pixels() == painted, "Gesture recall must preserve every artwork pixel in \(theme)")
            }
            try await invoke("fit_canvas")
            before = store.state["camera"]
            try await send(.leftMouseDown, lasso[0]); try await send(.leftMouseDragged, lasso[1])
            canvas.scrollWheel(with: event)
            for rotating in [false, true] {
                try await gesture(rotating, phase: .began)
                try await gesture(rotating, phase: .changed, delta: rotating ? 15 : 0.25)
                try await gesture(rotating, phase: .ended)
            }
            try await flush()
            try require(sameCamera(before), "Navigation cannot move the camera during a captured primary contact")
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
            let resumedGesture = store.state["camera"]
            try await gesture(false, phase: .began)
            try await gesture(false, phase: .changed, delta: 0.10)
            try await gesture(false, phase: .changed, delta: 0.05)
            try await gesture(false, phase: .ended)
            try await recall(resumedGesture, completed: store.state["camera"])
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
            do {
                let paperOnly = CommandLine.arguments.contains("--paper")
                if paperOnly || CommandLine.arguments.contains("--pen-response") {
                    Task { try await Task.sleep(for:.seconds(240)); note("FAIL native fixture deadline; retain private artifacts"); exit(1) }
                }
                for platform: UInt32 in paperOnly ? [1] : [0, 1] { try await run(platform) }
                exit(0)
            }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
