import Foundation
import QuartzCore
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// Production recovery coordinator with retained 16-bit source data, painted
/// tiles and a revisable correction/mask. All files and scenes are disposable.
@main struct SDRRecoveryChecks {
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    static func io<T>(_ work: @escaping () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { done in
            NativeProjectTask.io.async { done.resume(with: Result(catching: work)) }
        }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            store.edit(action) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
            }
        }
    }
    @MainActor static func wait(_ label: String, _ store: EditorStore, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(45)
        while !ready() {
            try require(Date() < deadline && store.failure == nil, store.failure ?? "Timed out: \(label)")
            // Pending placement cannot be captured for recovery until Apply.
            // Poll ordinary frames; explicit flush below tests durability.
            let now = FrameTrace.now()
            await withCheckedContinuation { done in
                store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() }
            }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func attach(_ store: EditorStore) async throws -> CAMetalLayer {
        let layer = CAMetalLayer(); layer.bounds = CGRect(x: 0, y: 0, width: 1024, height: 768)
        store.native!.attach(layer, width: 1024, height: 768, scale: 1)
        let deadline = Date().addingTimeInterval(45)
        while !store.snapshot["shaders_ready"].bool || store.workspaceLibrary?.ready != true {
            try require(Date() < deadline && store.failure == nil, store.failure ?? "Native startup")
            let now = FrameTrace.now()
            await withCheckedContinuation { done in store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
            try await Task.sleep(for: .milliseconds(10))
        }
        return layer
    }
    @MainActor static func flush(_ store: EditorStore) async throws {
        let saved = await withCheckedContinuation { done in store.flushPersistence { done.resume(returning: $0) } }
        try require(saved, store.recovery.error ?? store.storageFailure ?? "Full persistence barrier failed")
    }
    static func source(_ url: URL) throws -> Data {
        var codes = [UInt16]()
        for index in 0..<64 * 48 { codes += [UInt16(12000 + index), 41001, 60003, 65535] }
        let data = codes.withUnsafeBytes { Data($0) }
        let image = CGImage(width: 64, height: 48, bitsPerComponent: 16, bitsPerPixel: 64, bytesPerRow: 64 * 8,
            space: CGColorSpace(name: CGColorSpace.displayP3)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue).union(.byteOrder16Little),
            provider: CGDataProvider(data: data as CFData)!, decode: nil, shouldInterpolate: false, intent: .relativeColorimetric)!
        let output = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(output, image, nil)
        try require(CGImageDestinationFinalize(output), "Encode the source photo")
        let reopened = CGImageSourceCreateImageAtIndex(CGImageSourceCreateWithURL(url as CFURL, nil)!, 0, nil)!
        try require(reopened.bitsPerComponent == 16, "Source fixture must retain 16-bit samples")
        return try Data(contentsOf: url)
    }
    /// Compare the entire portable manifest and compressed payload, allowing
    /// only the new session's document revision to change on recovery adoption.
    static func archive(_ data: Data) throws -> (JSON, Data) {
        try require(data.count > 52, "Missing project header")
        let count = data[12..<20].enumerated().reduce(UInt64(0)) { $0 | UInt64($1.element) << ($1.offset * 8) }
        try require(count <= data.count - 52, "Invalid project metadata length")
        let end = 52 + Int(count)
        let manifest = try JSON.decode(String(decoding: data[52..<end], as: UTF8.self))
        return (manifest.replacing("document", with: manifest["document"].replacing("revision", with: JSON(0))), data.subdata(in: end..<data.count))
    }
    @MainActor static func run(platform: UInt32, space: String, depth: String, root: URL) async throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let photoURL = root.appendingPathComponent("Original.png"), photo = try source(photoURL)
        let files = RecoveryFiles(root: root), scene = UUID().uuidString
        var store: EditorStore? = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
        let layer = try await attach(store!)
        store!.projectFiles = ProjectFiles(store: store!, dialogs: .init(open: { _, done in done([]) }, save: { _, _, done in done(nil) }, create: { _, done in
            done(JSON(["extent": [128, 96], "color": ["space": space, "depth": depth], "background": "White"]))
        }, paste: { $0(.success([PhotoItem { $0(.success(.image(photo))) }])) }))
        func invoke(_ command: String) async throws { try await edit(store!, ["type": "invoke", "command": command]) }
        func idle() async throws {
            try await wait("Document completion", store!) { !store!.projectFiles.busy && store!.state["requests"].array.isEmpty }
            try require(store!.projectFiles.error == nil, store!.projectFiles.error ?? "")
        }
        try await invoke("new_document"); try await idle()
        try await invoke("paste_image"); try await idle()
        try await invoke("apply_transform"); try await idle()
        try await edit(store!, ["type": "color", "action": ["op": "definition", "color": ["space": "DisplayP3", "rgba": [0.8, 0.25, 0.1, 0.5]]]])
        try await invoke("select_all")
        try await edit(store!, ["type": "layer", "action": ["op": "fill_selection"]])
        try await invoke("deselect")
        try await edit(store!, ["type": "effect", "action": ["op": "insert", "effect": "exposure"]])
        let effect = store!.state["layer_tools"]["editing_layer"]["id"].uint
        try await edit(store!, ["type": "effect", "action": ["op": "set", "layer": effect, "key": "exposure", "value": ["kind": "number", "value": 0.75]]])
        try await edit(store!, ["type": "layer", "action": ["op": "add_mask", "id": effect, "replace": false]])
        let paint = store!.state["colors"]["foreground"].stableKey
        // The scene boundary blurs input and suspends workspace ownership.
        // No drawable/frame follows these edits; recovery must prepare them.
        store!.input(["type": "blur"]); store!.workspaceLibrary!.suspend()
        try await flush(store!)
        try require(store!.state["document_file"]["modified"].bool, "Recovery must not mark the document saved")
        let record = try await io { try files.list().records.first! }
        let original = try await io { try Data(contentsOf: files.archive(record)!) }
        let baseline = try archive(original)
        if platform == 0, let destination = ProcessInfo.processInfo.environment["CAPY_SDR_RECOVERY_FIXTURES"] {
            let directory = URL(fileURLWithPath: destination, isDirectory: true)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try original.write(to: directory.appendingPathComponent("\(space)-\(depth).capy"), options: .atomic)
            try photo.write(to: directory.appendingPathComponent("Original-16bit-P3.png"), options: .atomic)
        }
        try require(baseline.0["document"]["color"]["space"].string == space && baseline.0["document"]["color"]["depth"].string == depth, "Retain document space/depth")
        try await store!.workspaceLibrary!.resume()
        weak let released = store
        store = nil
        let deadline = Date().addingTimeInterval(10)
        while released != nil { try require(Date() < deadline, "Released editor retained"); try await Task.sleep(for: .milliseconds(10)) }
        let restored = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
        let restoredLayer = try await attach(restored)
        try await flush(restored)
        try require(try Data(contentsOf: files.archive(record)!) == original, "Blank scene startup must preserve the previous drawing")
        restored.recovery.restore(record)
        try await wait("Restore retained photo", restored) { restored.state["document_file"]["epoch"].uint == 1 && !restored.projectFiles.busy }
        try require(restored.projectFiles.error == nil, restored.projectFiles.error ?? "")
        try require(restored.state["colors"]["foreground"].stableKey == paint, "Restore tagged workspace paint")
        try await flush(restored)
        let recovered = try await io { try files.list().records.first! }
        let actual = try archive(try await io { try Data(contentsOf: files.archive(recovered)!) })
        try require(actual.0.stableKey == baseline.0.stableKey && actual.1 == baseline.1, "Recovery must preserve every source/profile, integer paint tile, correction and mask byte")
        try await edit(restored, ["type": "effect", "action": ["op": "set", "layer": effect, "key": "exposure", "value": ["kind": "number", "value": 1.25]]])
        try await edit(restored, ["type": "invoke", "command": "undo"])
        try await flush(restored)
        let undone = try await io { try files.list().records.first! }
        let undo = try archive(try await io { try Data(contentsOf: files.archive(undone)!) })
        try require(undo.0.stableKey == baseline.0.stableKey && undo.1 == baseline.1, "Recovered correction stays revisable with exact one-step Undo")
        try require(try Data(contentsOf: photoURL) == photo, "Original photo remains unchanged")
        print("PASS platform \(platform), \(space)/\(depth): suspended no-drawable flush, fresh owner, exact source/profile/paint/mask recovery, paint settings and continued correction Undo")
        withExtendedLifetime((layer, restoredLayer)) {}
    }
    @MainActor static func main() async throws {
        setbuf(stdout, nil)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-sdr-recovery-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        for platform: UInt32 in [0, 1] {
            for (space, depth) in [("DisplayP3", "U8"), ("ProPhoto", "U16")] {
                try await run(platform: platform, space: space, depth: depth, root: root.appendingPathComponent("\(platform)-\(space)"))
            }
        }
    }
}
