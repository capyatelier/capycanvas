import UIKit

private final class Contact: UITouch {
    var point = CGPoint(x: 500, y: 400)
    var time = ProcessInfo.processInfo.systemUptime
    var device: UITouch.TouchType = .pencil
    var updateIndex: NSNumber? = 41
    var expected = UITouch.Properties(rawValue: 1 | 16)
    var pressure: CGFloat = 0.2
    var roll: CGFloat = 0.3
    override var type: UITouch.TouchType { device }
    override var timestamp: TimeInterval { time }
    override func preciseLocation(in view: UIView?) -> CGPoint { point }
    override var estimationUpdateIndex: NSNumber? { updateIndex }
    override var estimatedPropertiesExpectingUpdates: UITouch.Properties { expected }
    override var force: CGFloat { pressure }
    override var maximumPossibleForce: CGFloat { 1 }
    override var altitudeAngle: CGFloat { .pi / 2 }
    override func azimuthAngle(in view: UIView?) -> CGFloat { 0 }
    override var rollAngle: CGFloat { roll }
}
@MainActor private final class PencilEstimateChecks: NSObject, UIApplicationDelegate {
    private var completedGroups = 0
    private let runID = ProcessInfo.processInfo.environment["CAPY_INPUT_RUN"] ?? UUID().uuidString
    func application(_ application: UIApplication,
        didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        // The fixture supplies its own canvas and Metal surface. Run from app
        // launch so a previously restored editor scene cannot suppress the check.
        Task { @MainActor in
            do {
                try await run(); try report(nil)
                print("PASS: UIKit delayed Pencil update correlation"); exit(0)
            } catch {
                try? report(error.localizedDescription)
                print("FAIL: \(error.localizedDescription)"); exit(1)
            }
        }
        return true
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
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-estimates-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
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
        try await flush()
        let contact = Contact()
        canvas.touchesBegan([contact], with: nil)
        try await flush()
        try require(canvas.estimates.pending.count == 1, "Pencil sample must await its sensor update")
        let original = canvas.estimates.pending.values.first!
        // UIKit correlates updates by estimationUpdateIndex. A later callback
        // timestamp must not leave the original stroke sample unresolved.
        let update = Contact(); update.time = contact.time + 0.02
        update.pressure = 0.8; update.roll = 1.7; update.expected = UITouch.Properties(rawValue: 16)
        canvas.touchesEstimatedPropertiesUpdated([update])
        try await flush()
        try require(canvas.estimates.pending.values.first?.record[2] == 0.8,
            "Delayed Pencil correction was lost when its callback timestamp changed")
        try require(canvas.estimates.pending.values.first?.record[7] == original.record[7],
            "A correction must retain the observation's original time")
        passed("Partial update uses the stable index and original observation time")
        update.time += 0.02; update.pressure = 0.1; update.roll = 2.1; update.expected = []
        canvas.touchesEstimatedPropertiesUpdated([update])
        try await flush()
        try require(canvas.estimates.pending.isEmpty, "Final update must release the retained estimate")
        passed("Final update releases the pending stroke sample")
        contact.time += 0.05; contact.point.x += 40; contact.updateIndex = 42; contact.expected = .force
        canvas.touchesMoved([contact], with: nil)
        try await flush()
        let next = canvas.estimates.pending.values.first!
        canvas.touchesEstimatedPropertiesUpdated([update])
        try await flush()
        try require(canvas.estimates.pending.count == 1 && canvas.estimates.pending.values.first!.record == next.record,
            "A retired update index must not alter a later observation")
        passed("Retired indices cannot redirect updates to new ink")
        contact.expected = []; contact.time += 0.01
        canvas.touchesEstimatedPropertiesUpdated([contact])
        canvas.touchesEnded([contact], with: nil)
        try await flush()
        try require(canvas.estimates.pending.isEmpty && canvas.contacts.isEmpty, "Cleanly finish corrected Pencil contact")
        passed("Completed contact retains no pending observations")
    }
}
@main struct CanvasPencilEstimateChecks {
    static func main() { UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(PencilEstimateChecks.self)) }
}
