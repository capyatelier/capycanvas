import SwiftUI

@MainActor final class CameraReadout: ObservableObject {
    @Published var value = JSON()
}

@MainActor final class EditorStore: ObservableObject {
    @Published private var structuralSnapshot = JSON()
    private var currentState = JSON()
    let camera = CameraReadout()
    @Published var catalog = JSON()
    @Published var failure: String?
    @Published var canvasSubmitted = false
    /// Measured native window controls; editor geometry otherwise comes from Rust.
    @Published var headerLeadingInset: CGFloat = 0
    var cameraRevision: UInt64 = 0
    var wake: (() -> Void)?
    private(set) var native: NativeOwner?
    lazy var layerThumbnails = LayerThumbnails(store: self)
    var snapshot: JSON { structuralSnapshot.replacing("state", with: currentState) }
    var state: JSON { currentState }

    init(platform: UInt32) {
        do {
            native = try NativeOwner(platform: platform) { [weak self] snapshot, failure in
                DispatchQueue.main.async { self?.receive(snapshot, failure) }
            }
            native?.submit(2, JSON(["type": "catalog"])) { [weak self] result in
                DispatchQueue.main.async { self?.catalog = result ?? JSON() }
            }
            #if DEBUG
            // Deterministic editor fixtures exercise actions directly, without
            // driving platform menu bars. Production builds have no override.
            if let source = ProcessInfo.processInfo.environment["CAPY_INITIAL_ACTIONS"] {
                for action in try JSON.decode(source).array { native?.submit(0, action) }
            }
            #endif
        } catch { failure = error.localizedDescription }
    }
    private func receive(_ next: JSON?, _ error: String?) {
        if let error { failure = error }
        if let next {
            if !next["state"].isNull {
                currentState = next["state"]
                structuralSnapshot = next
                camera.value = state["camera"]
            }
            else if !next["camera"].isNull {
                // Camera patches update the readout alone; dragging the canvas
                // must not rebuild every panel and brush preview at input rate.
                currentState = state.replacing("camera", with: next["camera"]).replacing("revision", with: next["revision"])
                camera.value = next["camera"]
            }
            cameraRevision = state["camera"]["revision"].uint
        }
        wake?()
    }
    func dispatch(_ action: JSON) { native?.submit(0, action); wake?() }
    func dispatch(_ value: [String: Any]) { dispatch(JSON(value)) }
    func edit(_ value: [String: Any], completion: @escaping @MainActor (String?) -> Void) {
        guard let native else { completion("The canvas session is unavailable"); return }
        native.edit(JSON(value)) { error in DispatchQueue.main.async { completion(error) } }
        wake?()
    }
    func invoke(_ command: String) { dispatch(["type": "invoke", "command": command]) }
    func layer(_ action: [String: Any]) { dispatch(["type": "layer", "action": action]) }
    func importLayer(_ url: URL) { native?.importLayer(url); wake?() }
    func customize(_ action: [String: Any]) { dispatch(["type": "customize", "action": action]) }
    func input(_ value: [String: Any]) { native?.submit(1, JSON(value)); wake?() }
    func command(_ id: String) -> JSON { state["commands"].array.first { $0["id"].string == id } ?? JSON() }
    func query(_ value: [String: Any], completion: @escaping @MainActor (JSON) -> Void) {
        guard let native else { completion(JSON()); return }
        native.submit(2, JSON(value)) { result in DispatchQueue.main.async { completion(result ?? JSON()) } }
    }
    func numeric(_ control: JSON, value: Double, operation: [String: Any], completion: @escaping @MainActor (JSON) -> Void) {
        do { completion(try resolveNumber(control, value: value, operation: operation)) }
        catch { failure = error.localizedDescription }
    }
    func resolveNumber(_ control: JSON, value: Double, operation: [String: Any]) throws -> JSON {
        let request = try JSON(["control": control.raw, "value": value, "operation": operation]).encoded()
        guard let response = request.withCString({ capy_apple_numeric($0) }) else { throw HostFailure(message: "Numeric input failed") }
        defer { capy_apple_string_free(response) }
        let value = try JSON.decode(String(cString: response))
        if !value["error"].isNull { throw HostFailure(message: value["error"].string) }
        return value
    }
}
