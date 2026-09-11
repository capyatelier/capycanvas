import SwiftUI
import CoreGraphics

/// GPU-generated previews only. The cache is separate from editor snapshots,
/// requests visible rows, caps in-flight readbacks at eight, and sleeps at idle.
@MainActor final class LayerThumbnails: ObservableObject {
    @Published private(set) var images: [String: CGImage] = [:]
    private weak var store: EditorStore?
    private var visible: Set<UInt64> = []
    private struct Version: Equatable { let target: UInt64; let revision: UInt64 }
    private struct Request { let key: String; let version: Version }
    private var completed: [String: Version] = [:]
    private var pending: [UInt64: Request] = [:]
    private var next: UInt64 = 0
    private var task: Task<Void, Never>?
    private var generation: UInt64 = 0

    init(store: EditorStore) { self.store = store }
    func reset() {
        generation &+= 1; task?.cancel(); task = nil
        images.removeAll(); completed.removeAll(); pending.removeAll(); refresh()
    }
    static func key(_ id: UInt64, _ mask: Bool) -> String { "\(id):\(mask)" }
    func show(_ id: UInt64) { visible.insert(id); refresh() }
    func hide(_ id: UInt64) { visible.remove(id) }
    func refresh() {
        guard task == nil else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .milliseconds(120)) } catch { return }
                guard let self else { return }
                let more = await self.poll()
                if Task.isCancelled { return }
                if !more { self.task = nil; return }
            }
        }
    }
    private func desired() -> [String: Version] {
        var result: [String: Version] = [:]
        for layer in store?.state["layers"].array ?? [] where visible.contains(layer["id"].uint) {
            for mask in [false, true] {
                if mask ? !layer["has_mask"].bool : layer["group"].bool || !layer["content_icon"].isNull { continue }
                result[Self.key(layer["id"].uint, mask)] = Version(
                    target: layer[mask ? "mask_id" : "id"].uint,
                    revision: layer[mask ? "mask_revision" : "paint_revision"].uint)
            }
        }
        return result
    }
    private func poll() async -> Bool {
        let generation = generation
        guard let store else { return false }
        let wanted = desired()
        var requests: [UInt64: Request] = [:]
        for key in wanted.keys.sorted() {
            guard pending.count + requests.count < 8 else { break }
            let version = wanted[key]!
            if completed[key] == version || pending.values.contains(where: { $0.key == key }) { continue }
            next &+= 1; requests[next] = Request(key: key, version: version)
        }
        let ids = Set(store.state["layers"].array.map { String($0["id"].uint) })
        let obsolete = images.keys.filter { !ids.contains(String($0.split(separator: ":")[0])) }
        for key in obsolete { images.removeValue(forKey: key); completed.removeValue(forKey: key) }
        guard !requests.isEmpty || !pending.isEmpty else { return false }
        let reply: JSON = await withCheckedContinuation { continuation in
            store.query(["type": "layer_thumbnails", "requests": requests.map { [$0.key, $0.value.version.target] }]) {
                continuation.resume(returning: $0)
            }
        }
        guard generation == self.generation else { return false }
        if reply.isNull { pending.removeAll(); return false }
        for token in reply["accepted"].array { if let request = requests[token.uint] { pending[token.uint] = request } }
        let current = desired()
        var updated = images
        for image in reply["images"].array {
            guard let request = pending.removeValue(forKey: image[0].uint), current[request.key] == request.version else { continue }
            let width = Int(image[1].uint), height = Int(image[2].uint)
            guard width > 0, height > 0, width <= 256, height <= 256 else { continue }
            let bytes = Data(image[3].array.map { UInt8(clamping: Int($0.uint)) })
            guard bytes.count == width * height * 4,
                let provider = CGDataProvider(data: bytes as CFData), let space = CGColorSpace(name: CGColorSpace.sRGB),
                let bitmap = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                    bytesPerRow: width * 4, space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                    provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else { continue }
            updated[request.key] = bitmap; completed[request.key] = request.version
        }
        if !reply["images"].array.isEmpty { images = updated }
        return !pending.isEmpty || desired().contains { completed[$0.key] != $0.value }
    }
}
