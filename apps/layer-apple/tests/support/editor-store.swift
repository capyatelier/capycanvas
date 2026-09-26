import Foundation
import QuartzCore

func require(_ value: Bool, _ message: String) throws {
    if !value { throw HostFailure(message: message) }
}

@MainActor func wait(_ label: @autoclosure () -> String, seconds: TimeInterval = 45, failure: () -> String? = { nil },
    step: () async throws -> Void = {}, _ ready: () -> Bool) async throws {
    let deadline = Date().addingTimeInterval(seconds)
    while !ready() {
        if let message = failure() { throw HostFailure(message: message) }
        try require(Date() < deadline, "Timed out: \(label())")
        try await step()
        try await Task.sleep(for: .milliseconds(10))
    }
}

@MainActor func frame(_ native: NativeOwner) async {
    let now = FrameTrace.now()
    await withCheckedContinuation { done in native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
}

@MainActor func prepare(_ native: NativeOwner, _ label: String) async throws {
    let prepared = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
    try require(prepared, "Prepare canvas: \(label)")
}

@MainActor func attachSurface(_ store: EditorStore, _ size: CGSize) -> CAMetalLayer {
    let surface = CAMetalLayer()
    surface.bounds = CGRect(origin: .zero, size: size)
    store.native!.attach(surface, width: UInt32(size.width), height: UInt32(size.height), scale: 1)
    return surface
}

extension EditorStore {
    func apply(_ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            edit(action) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
            }
        }
    }
}
