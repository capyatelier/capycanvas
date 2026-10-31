// Standalone checks against the actual shared frame driver. No UI automation.
import Foundation

@MainActor final class EditorStore {
    let native: NativeOwner? = NativeOwner()
    let bubbles = WorkspaceBubbles()
    var cameraRevision: UInt64 = 0
    var canvasSubmitted = false
}
@MainActor final class WorkspaceBubbles {
    var needsFrame = false
    var ticks: [TimeInterval] = []
    func tick(_ now: TimeInterval) { ticks.append(now) }
}
@MainActor final class NativeOwner {
    var admissions: [Bool] = []
    var denials: [UInt64] = []
    var framePublicationActive = false
    func framePublication(active: Bool) { framePublicationActive = active }
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
        do {
            let store = EditorStore()
            let probe = CanvasFrameDriver(store: store)
            store.canvasSubmitted = true
            probe.deactivate()
            await drainMainQueue()
            assert(store.canvasSubmitted, "A never-active probe must not reset another view's submitted canvas")
        }

        let frames = CanvasFrameDriver(store: store)
        var paused = true, submissions = 0
        frames.setPaused = { paused = $0 }
        frames.submittedViewport = { submissions += 1 }
        assert(!store.native!.framePublicationActive, "Unattached canvases must publish without a display frame")
        frames.activate()
        assert(store.native!.framePublicationActive, "An active display link can publish motion on its next frame")
        frames.tick(target: 1); frames.tick(target: 2)
        assert(store.native!.completions.count == 1, "Only one frame may be queued")
        assert(store.native!.denials.suffix(2) == [0, 2])
        assert(store.bubbles.ticks.count == 1, "Rejected duplicate ticks must not advance bubble motion")
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
        assert(!store.native!.framePublicationActive, "Detaching must flush pending motion without another frame")
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
        var visible = false
        frames.canPresent = { visible }
        frames.wake(); frames.tick(target: 6)
        assert(!store.native!.framePublicationActive, "Occluded canvases must publish without a display frame")
        assert(paused && store.native!.completions.isEmpty, "Hidden wake and stale ticks must not submit frames")
        assert(store.canvasSubmitted && store.cameraRevision == 4 && submissions == 2,
            "Visibility changes must retain the submitted canvas and camera")

        visible = true; frames.wake(); frames.tick(target: 7)
        assert(!paused && store.native!.completions.count == 1, "Visible wake must resume frame admission")
        store.native!.complete(revision: 5)
        await drainMainQueue()
        assert(paused && store.canvasSubmitted && store.cameraRevision == 5 && submissions == 2)

        frames.wake(); frames.tick(target: 8)
        visible = false; frames.wake(); frames.tick(target: 9)
        assert(paused && store.native!.completions.count == 1, "Hiding must reject ticks while a frame is pending")
        store.native!.complete(again: true, revision: 6)
        await drainMainQueue()
        assert(paused && store.canvasSubmitted && store.cameraRevision == 6 && submissions == 2,
            "A pending completion cannot wake a hidden window or reset its canvas")

        visible = true; frames.wake(); frames.tick(target: 10)
        assert(!paused && store.native!.completions.count == 1)
        store.native!.complete(revision: 7)
        await drainMainQueue()
        assert(paused && store.cameraRevision == 7 && store.canvasSubmitted && submissions == 2)
        do {
            let store = EditorStore(), native = store.native!
            let frames = CanvasFrameDriver(store: store)
            var paused = true
            frames.setPaused = { paused = $0 }
            frames.activate()
            store.bubbles.needsFrame = true
            frames.tick(target: 11)
            native.complete()
            await drainMainQueue()
            assert(!paused && store.bubbles.ticks.count == 1,
                "Active bubble motion must keep the frame clock running after an idle canvas reply")
            frames.tick(target: 12)
            store.bubbles.needsFrame = false
            native.complete()
            await drainMainQueue()
            assert(paused && store.bubbles.ticks.count == 2 && native.completions.isEmpty,
                "Settled bubbles and an idle canvas must stop the frame clock")
        }
        do {
            let store = EditorStore(), native = store.native!
            let frames = CanvasFrameDriver(store: store)
            var sceneActive = true, paused = true
            frames.canPresent = { sceneActive }
            frames.setPaused = { paused = $0 }
            frames.activate()
            frames.tick(target: 11)
            sceneActive = false
            native.complete(again: true)
            await drainMainQueue()
            assert(!native.framePublicationActive, "Scene deactivation must flush pending motion without another tick")
            assert(paused && native.admissions == [true] && native.completions.isEmpty,
                "A pending completion must stay paused after scene deactivation without another tick")
            sceneActive = true
            frames.wake()
            assert(!paused, "Opening the scene gate must wake the paused display link")
            frames.tick(target: 12)
            assert(native.admissions == [true, true] && native.completions.count == 1,
                "The first real tick after scene activation must submit exactly one frame")
            native.complete()
            await drainMainQueue()
            assert(paused && store.canvasSubmitted && native.admissions == [true, true],
                "The resumed idle frame must preserve canvas readiness without free-running")
        }
        print("Shared frame-driver checks passed: admission, wake, detach, replacement, visibility and bubble motion")
    }
}
