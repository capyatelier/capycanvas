// Standalone checks against the actual shared frame driver. No UI automation.
import Foundation

@MainActor final class EditorStore {
    let native: NativeOwner? = NativeOwner()
    var cameraRevision: UInt64 = 0
    var canvasSubmitted = false
}
@MainActor final class NativeOwner {
    var canAdmitPresentation = true
    var admissions: [Bool] = []
    var denials: [UInt64] = []
    func observeTick(now: UInt64, target: UInt64, admitted: Bool, denial: UInt64) {
        admissions.append(admitted); denials.append(denial)
    }
    func observeActivity(active: Bool) {}
    var completions: [@Sendable (Bool, UInt64, [UInt64]) -> Void] = []
    func frame(now: UInt64, target: UInt64,
        completion: @escaping @Sendable (Bool, UInt64, [UInt64]) -> Void) {
        completions.append(completion)
    }
    func complete(again: Bool = false, revision: UInt64 = 1) {
        completions.removeFirst()(again, revision, [1, 1, 1, 1, 1])
    }
}

@main struct FrameDriverChecks {
    @MainActor static func drainMainQueue() async {
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
    }
    @MainActor static func main() async {
        let store = EditorStore(), driver = CanvasFrameDriver(store: EditorStore())
        // A driver must not retain an abandoned editor window.
        driver.activate(); driver.tick(target: 1)

        let frames = CanvasFrameDriver(store: store)
        var paused = true, submissions = 0
        frames.setPaused = { paused = $0 }
        frames.submittedViewport = { submissions += 1 }
        frames.activate()
        store.native!.canAdmitPresentation = false
        frames.tick(target: 0.5)
        assert(store.native!.completions.isEmpty && store.native!.admissions == [false])
        assert(store.native!.denials == [3])
        assert(!paused, "A full drawable pool must keep the link awake for retry")
        frames.wake()
        store.native!.canAdmitPresentation = true
        frames.tick(target: 1); frames.tick(target: 2)
        assert(store.native!.completions.count == 1, "Only one frame may be queued")
        assert(store.native!.denials.suffix(2) == [0, 2])
        frames.wake()
        store.native!.complete()
        await drainMainQueue()
        assert(!paused, "Input arriving during a frame must survive an idle reply")
        assert(store.canvasSubmitted && submissions == 1)

        frames.tick(target: 2.5)
        store.native!.complete(again: true)
        await drainMainQueue()
        assert(submissions == 1, "Canvas readiness must not publish accessibility changes on every frame")

        frames.tick(target: 3)
        frames.deactivate()
        store.native!.complete(revision: 2)
        await drainMainQueue()
        assert(paused && !store.canvasSubmitted, "Detached views must stay asleep")
        assert(store.cameraRevision == 1 && submissions == 1)

        frames.activate(); frames.tick(target: 4)
        frames.deactivate(); frames.activate()
        store.native!.complete(revision: 3)
        await drainMainQueue()
        assert(!paused && !store.canvasSubmitted, "Old submissions cannot reveal a replacement surface")
        assert(store.cameraRevision == 1 && submissions == 1)
        frames.tick(target: 5)
        store.native!.complete(revision: 4)
        await drainMainQueue()
        assert(paused && store.canvasSubmitted && submissions == 2)
        assert(store.cameraRevision == 4)
        print("Shared frame-driver checks passed: drawable backpressure, admission, wake, detach and replacement")
    }
}
