import Foundation
import QuartzCore

/// Shared frame admission for UIKit and AppKit. Platform views own display links;
/// the serial native owner executes work. A completion is submission, not display.
@MainActor final class CanvasFrameDriver {
    private weak var store: EditorStore?
    private var generation: UInt64 = 0
    private var pending = false
    private var active = false
    var setPaused: (Bool) -> Void = { _ in }
    var submittedViewport: () -> Void = {}

    init(store: EditorStore) { self.store = store }
    func activate() { active = true; wake() }
    func deactivate() { active = false; generation &+= 1; setPaused(true) }
    func wake() { generation &+= 1; if active { setPaused(false) } }
    func tick(target: TimeInterval) {
        guard active, !pending, let native = store?.native else { return }
        pending = true
        let submittedGeneration = generation
        native.frame(now: UInt64(CACurrentMediaTime() * 1_000_000_000),
            target: UInt64(max(0, target) * 1_000_000_000)) { [weak self] again, revision, costs in
            DispatchQueue.main.async {
                guard let self else { return }
                self.pending = false
                self.store?.cameraRevision = revision
                guard self.active else { return }
                if costs[2] > 0 { self.submittedViewport() }
                // A resize/input/wake during the queued frame must survive its
                // idle reply, including a surface detached and attached again.
                self.setPaused(!again && self.generation == submittedGeneration)
            }
        }
    }
}
