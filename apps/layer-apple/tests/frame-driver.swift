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
    var available: (@Sendable () -> Void)?
    var retries: [UInt64] = []
    func whenPresentationAvailable(_ action: (@Sendable () -> Void)?) { available = action }
    func observeFrameRetry(now: UInt64, target: UInt64, admitted: Bool, denial: UInt64) {
        retries.append(denial)
    }
    func releaseCapacity() {
        canAdmitPresentation = true
        let action = available; available = nil; action?()
    }
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
        await checkPresentationRetry()
        print("Shared frame-driver checks passed: drawable backpressure, admission, wake, detach and replacement")
    }
    @MainActor static func checkPresentationRetry() async {
        let store = EditorStore(), native = store.native!
        var now: TimeInterval = 100
        let driver = CanvasFrameDriver(store: store, currentTime: { now })
        driver.activate()
        native.canAdmitPresentation = false
        driver.tick(target: 101)
        assert(native.available != nil && native.completions.isEmpty)
        native.releaseCapacity()
        await drainMainQueue()
        assert(native.completions.count == 1 && native.retries == [0])
        assert(native.admissions == [false], "A presentation retry is not a display tick")
        native.complete(again: true); await drainMainQueue()

        // A delayed main-queue delivery cannot submit past its original target.
        native.canAdmitPresentation = false
        driver.tick(target: 101)
        now = 101; native.releaseCapacity(); await drainMainQueue()
        assert(native.completions.isEmpty && native.retries == [0])

        // A newer tick and its submission supersede an already queued callback.
        native.canAdmitPresentation = false
        driver.tick(target: 102)
        let old = native.available!
        native.canAdmitPresentation = true
        driver.tick(target: 103); old(); await drainMainQueue()
        assert(native.completions.count == 1 && native.retries == [0])
        native.complete(again: true); await drainMainQueue()

        // A callback from a removed surface cannot render its replacement.
        native.canAdmitPresentation = false
        driver.tick(target: 104)
        let detached = native.available!
        driver.deactivate(); driver.activate()
        native.canAdmitPresentation = true; detached(); await drainMainQueue()
        assert(native.completions.isEmpty && native.retries == [0])

        // If capacity disappeared again, do not register a retry loop.
        native.canAdmitPresentation = false
        driver.tick(target: 105)
        native.releaseCapacity(); native.canAdmitPresentation = false
        await drainMainQueue()
        assert(native.completions.isEmpty && native.available == nil && native.retries == [0, 3])
        driver.deactivate()
        print("Presentation retry checks passed: deadline, superseding tick, detached surface and bounded retry")
    }
}
