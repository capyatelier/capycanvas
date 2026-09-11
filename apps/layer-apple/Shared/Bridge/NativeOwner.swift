import Foundation
import QuartzCore

/// ARC lease crossing the queue boundary. UIKit/AppKit owns view geometry;
/// only the render owner uses the layer's Metal surface and drawable APIs.
private final class MetalLayerLease: @unchecked Sendable {
    let value: CAMetalLayer
    init(_ value: CAMetalLayer) { self.value = value }
}

/// The only owner of Rust and GPU state. UI callbacks submit owned input batches.
final class NativeOwner: @unchecked Sendable {
    private let queue: DispatchQueue
    private let handle: OpaquePointer
    private var layer: CAMetalLayer?
    private var lastSnapshotTime: UInt64 = 0
    private var bundledFiltersLoaded = false
    let receive: @Sendable (JSON?, String?) -> Void

    init(platform: UInt32, receive: @escaping @Sendable (JSON?, String?) -> Void) throws {
        let queue = DispatchQueue(label: "art.capycanvas.render", qos: .userInteractive)
        guard let handle = queue.sync(execute: { capy_apple_create(platform) }) else {
            throw HostFailure(message: "Could not create the native canvas session")
        }
        self.queue = queue; self.handle = handle; self.receive = receive
    }
    deinit {
        let handle = handle, retainedLayer = layer
        queue.async {
            capy_apple_detach(handle)
            capy_apple_destroy(handle)
            withExtendedLifetime(retainedLayer) {}
        }
    }
    private func check(_ result: Int32) throws {
        if result < 0 { throw HostFailure(message: capy_apple_error(handle).map(String.init(cString:)) ?? "Native operation failed") }
    }
    private func request(_ kind: UInt32, _ value: JSON = JSON()) throws -> JSON? {
        let source = try value.encoded()
        let result = source.withCString { capy_apple_request(handle, kind, $0) }
        guard let result else {
            if let error = capy_apple_error(handle) { throw HostFailure(message: String(cString: error)) }
            return nil
        }
        defer { capy_apple_string_free(result) }
        return try JSON.decode(String(cString: result))
    }
    private func publish() throws {
        if let snapshot = try request(3) { receive(snapshot, nil) }
    }
    private func perform(_ work: @escaping @Sendable () throws -> Void) {
        queue.async { [self] in
            do { try work() } catch { receive(nil, error.localizedDescription) }
        }
    }
    func submit(_ kind: UInt32, _ value: JSON, completion: (@Sendable (JSON?) -> Void)? = nil) {
        perform { [self] in
            let result = try request(kind, value)
            try publish()
            completion?(result)
        }
    }
    func attach(_ layer: CAMetalLayer, width: UInt32, height: UInt32, scale: Float) {
        let lease = MetalLayerLease(layer)
        perform { [self] in
            let layer = lease.value
            try check(capy_apple_attach(handle, Unmanaged.passUnretained(layer).toOpaque(), width, height, scale))
            self.layer = layer
            // Packages use the same manifest and WGSL as every other host.
            if !bundledFiltersLoaded, let url = Bundle.main.url(forResource: "manifest", withExtension: "json", subdirectory: "filters") {
                let manifest = try String(contentsOf: url, encoding: .utf8)
                let names = try request(2, JSON(["type": "filter_package_modules", "manifest": manifest]))?.array ?? []
                var modules: [String: String] = [:]
                for name in names {
                    modules[name.string] = try String(contentsOf: url.deletingLastPathComponent().appendingPathComponent(name.string), encoding: .utf8)
                }
                _ = try request(2, JSON(["type": "load_filter_package", "manifest": manifest, "modules": modules, "mode": "replace"]))
                bundledFiltersLoaded = true
            }
            try publish()
        }
    }
    func resize(width: UInt32, height: UInt32, scale: Float) {
        perform { [self] in
            try check(capy_apple_resize(handle, width, height, scale)); try publish()
        }
    }
    func detach() {
        perform { [self] in try check(capy_apple_detach(handle)); layer = nil }
    }
    func pointer(id: UInt64, tool: UInt32, button: UInt32, records: [Double], predicted: Bool, revision: UInt64) {
        perform { [self] in
            try records.withUnsafeBufferPointer {
                try check(capy_apple_pointer(handle, id, tool, button, $0.baseAddress, $0.count, predicted ? 1 : 0, revision))
            }
        }
    }
    func scroll(x: Float, y: Float, dx: Float, dy: Float, scale: Float, zoom: Bool, horizontal: Bool) {
        perform { [self] in
            try check(capy_apple_scroll(handle, x, y, dx, dy, scale, zoom ? 1 : 0, horizontal ? 1 : 0))
            try publish()
        }
    }
    func gesture(x: Float, y: Float, scale: Float, rotation: Float) {
        perform { [self] in
            try check(capy_apple_gesture(handle, x, y, scale, rotation))
            try publish()
        }
    }
    /// One frame may be outstanding. Completion never means drawable presentation.
    func frame(now: UInt64, target: UInt64, completion: @escaping @Sendable (Bool, UInt64, [UInt64]) -> Void) {
        queue.async { [self] in
            var costs = [UInt64](repeating: 0, count: 5)
            do {
                let result = capy_apple_frame(handle, now, max(now, target), &costs)
                try check(result)
                // Always flush the final state before the display link sleeps.
                // Throttling the pen-up frame can otherwise leave Undo/layers
                // stale indefinitely, until an unrelated action wakes the UI.
                if result == 0 || now >= lastSnapshotTime + 33_000_000 {
                    try publish(); lastSnapshotTime = now
                }
                completion(result == 1, capy_apple_camera_revision(handle), costs)
            } catch {
                receive(nil, error.localizedDescription)
                completion(false, capy_apple_camera_revision(handle), costs)
            }
        }
    }
}
