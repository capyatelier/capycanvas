import Foundation
import QuartzCore

/// Shared frame admission for UIKit and AppKit. Platform views own display links;
/// the serial native owner executes work. A completion is submission, not display.
@MainActor final class CanvasFrameDriver {
    private weak var store: EditorStore?
    private var generation: UInt64 = 0
    private var surfaceGeneration: UInt64 = 0
    private var pending = false
    private var active = false
    var setPaused: (Bool) -> Void = { _ in }
    var submittedViewport: () -> Void = {}

    init(store: EditorStore) { self.store = store }
    func activate() { surfaceGeneration &+= 1; resetSubmission(); active = true; wake() }
    func deactivate() {
        active = false; generation &+= 1; surfaceGeneration &+= 1
        resetSubmission(); setPaused(true)
    }
    private func resetSubmission() {
        let surface = surfaceGeneration
        // Attach/detach can happen inside native/SwiftUI layout. Publish outside
        // that pass, and never let an old surface reset its replacement.
        DispatchQueue.main.async { [weak self] in
            guard let self, self.surfaceGeneration == surface else { return }
            if self.store?.canvasSubmitted == true { self.store?.canvasSubmitted = false }
        }
    }
    func wake() { generation &+= 1; if active { setPaused(false) } }
    func tick(target: TimeInterval) {
        guard active, !pending, let native = store?.native else { return }
        pending = true
        let submittedGeneration = generation
        let submittedSurface = surfaceGeneration
        native.frame(now: UInt64(CACurrentMediaTime() * 1_000_000_000),
            target: UInt64(max(0, target) * 1_000_000_000)) { [weak self] again, revision, costs in
            DispatchQueue.main.async {
                guard let self else { return }
                self.pending = false
                guard self.active else { return }
                if submittedSurface == self.surfaceGeneration {
                    self.store?.cameraRevision = revision
                    if costs[2] > 0 {
                        if self.store?.canvasSubmitted != true { self.store?.canvasSubmitted = true }
                        self.submittedViewport()
                    }
                }
                // A resize/input/wake during the queued frame must survive its
                // idle reply, including a surface detached and attached again.
                self.setPaused(!again && self.generation == submittedGeneration)
            }
        }
    }
}
