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

extension WorkspaceController {
    func started(_ label: String = "workspace startup") async throws {
        try await wait(label, failure: { self.error }) { self.ready && !self.busy }
    }
    func settle(_ label: String = "workspace operation") async throws {
        await withCheckedContinuation { done in
            store!.native!.workspace(JSON(["type": "tick"])) { _, _ in DispatchQueue.main.async { done.resume() } }
        }
        try await wait(label, failure: { self.error }) { self.ready && !self.busy && !self.view["loading"].bool }
    }
    func perform(_ input: [String: Any]) async throws { send(input); try await settle() }
    func answer(_ form: [String: Any], name: String? = nil, description: String? = nil, choice: String? = nil) async throws {
        try await perform(["type": "form", "action": form])
        try require(!view["prompt"].isNull, "No form for \(form)")
        if let name { formName = name }
        if let description { formDescription = description }
        if let choice { formChoice = choice }
        submit(); try await settle()
        try require(view["prompt"].isNull, "Form stayed open: \(error ?? view["prompt"].stableKey)")
    }
    func create(_ name: String) async throws { try await answer(["type": "new"], name: name) }
    func flushed() async throws {
        try await settle()
        try await wait("workspace autosave", failure: { self.error }) { !self.view["dirty"].bool && !self.view["saving"].bool }
    }
    func closed() async throws {
        let saved = await withCheckedContinuation { done in close { done.resume(returning: $0) } }
        try require(saved, error ?? "Workspace close failed")
    }
    func detached() async { await withCheckedContinuation { done in detach { done.resume() } } }
    func workspaceRows() async throws -> [JSON] {
        try await perform(["type": "open", "page": "workspaces"])
        let rows = view["rows"].array
        try await perform(["type": "dismiss"])
        return rows
    }
    func capture() async throws -> JSON {
        try await withCheckedThrowingContinuation { done in
            store!.native!.workspace(JSON(["type": "capture"])) { value, error in
                DispatchQueue.main.async {
                    if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume(returning: value ?? JSON()) }
                }
            }
        }
    }
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
