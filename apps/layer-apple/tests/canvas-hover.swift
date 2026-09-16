import UIKit

// Run as a standalone UIKit application with the production Shared/iOS sources
// and native bridge. Supplied recognizer states test callback delivery to Rust;
// physical Pencil recognition and rendered cursor appearance remain separate.
private final class Hover: UIHoverGestureRecognizer {
    var phase: UIGestureRecognizer.State = .began
    override var state: UIGestureRecognizer.State { get { phase } set { phase = newValue } }
    override func location(in view: UIView?) -> CGPoint { CGPoint(x: 200, y: 150) }
    override var altitudeAngle: CGFloat { .pi / 3 }
    override func azimuthAngle(in view: UIView?) -> CGFloat { .pi / 4 }
    override var rollAngle: CGFloat { 0.2 }
    override var zOffset: CGFloat { 0.5 }
}

@MainActor private final class HoverChecks: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication,
        didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        Task { @MainActor in
            do { try await run(); print("PASS: UIKit hover exit, cancellation and next-hover recovery"); exit(0) }
            catch { print("FAIL: \(error.localizedDescription)"); exit(1) }
        }
        return true
    }
    private func run() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-hover-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        // Reuse the existing optional input recorder without a test-only hook.
        setenv("CAPY_TRACE_DIRECTORY", root.path, 1)
        setenv("CAPY_TRACE_SECONDS", "2", 1)
        setenv("CAPY_TRACE_GPU", "0", 1)
        let store = EditorStore(platform: 0, persistence: EditorPersistence(root: nil))
        let canvas = CanvasView(store: store)
        canvas.frame = CGRect(x: 0, y: 0, width: 600, height: 450)
        canvas.contentScaleFactor = 2
        defer { canvas.stop() }
        guard let native = store.native else { throw HostFailure(message: "No owner") }
        func flush() async {
            await withCheckedContinuation { continuation in
                native.submit(2, JSON(["type": "catalog"])) { _ in continuation.resume() }
            }
        }
        await flush()
        let hover = Hover()
        for phase: UIGestureRecognizer.State in [.began, .changed, .ended, .began, .cancelled] {
            hover.phase = phase; canvas.hovered(hover)
        }
        // A hover recognizer must not overwrite an active Pencil contact.
        let contact = UITouch()
        canvas.contacts[ObjectIdentifier(contact)] = PencilContact(id: 1, tool: 0, button: 0)
        hover.phase = .changed; canvas.hovered(hover)
        canvas.contacts.removeAll()
        for phase: UIGestureRecognizer.State in [.began, .ended] {
            hover.phase = phase; canvas.hovered(hover)
        }
        await flush()
        if let failure = store.failure { throw HostFailure(message: failure) }
        let deadline = Date().addingTimeInterval(8)
        var trace: URL?
        while trace == nil && Date() < deadline {
            trace = try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)
                .first { $0.pathExtension == "jsonl" }
            if trace == nil { try await Task.sleep(for: .milliseconds(20)) }
        }
        guard let trace else { throw HostFailure(message: "Input recorder did not finish") }
        let lines = try String(contentsOf: trace, encoding: .utf8).split(separator: "\n")
        let inputs = try lines.dropFirst().compactMap { line -> [UInt64]? in
            let values = try JSONSerialization.jsonObject(with: Data(line.utf8)) as! [NSNumber]
            return values[0].uint64Value == 2 ? values.map(\.uint64Value) : nil
        }
        guard inputs.map({ $0[8] }) == [0, 0, 4, 0, 4, 0, 4] else {
            throw HostFailure(message: "Hover cancellation must clear the cursor, suppress contact overlap and allow a fresh hover; received phases \(inputs.map { $0[8] })")
        }
        guard inputs.allSatisfy({ $0[6] == 1 && $0[7] == 0 && $0[9] == 0 && $0[10] == 1 }) else {
            throw HostFailure(message: "Each hover must reach Rust as one accepted real pen sample")
        }
        guard !store.command("undo")["enabled"].bool && !store.command("redo")["enabled"].bool else {
            throw HostFailure(message: "Hover must preserve empty artwork history")
        }
    }
}

@main struct CanvasHoverChecks {
    static func main() {
        UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(HoverChecks.self))
    }
}
