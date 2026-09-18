import SwiftUI
import CoreGraphics

struct FilterPreviewReply: Sendable {
    let status: JSON
    let atlas: NativeFilterPreviews?
    let error: String?
}

/// An immutable Rust allocation, independent of the live editor. Image creation
/// happens on a utility worker; the render owner only transfers its ownership.
final class NativeFilterPreviews: @unchecked Sendable {
    private let handle: OpaquePointer
    init(_ handle: OpaquePointer) { self.handle = handle }
    deinit { capy_filter_previews_free(handle) }
    struct Decoded: @unchecked Sendable {
        let request: UInt64
        let images: [String: CGImage]
    }
    func decode() -> Decoded? {
        var info = CapyFilterPreviewInfo()
        capy_filter_previews_read(handle, &info)
        guard let filters = info.filters, let pixels = info.pixels,
            let ids = try? JSON.decode(String(cString: filters)).array.map(\.string),
            !ids.isEmpty, ids.count <= 8 else { return nil }
        let width = Int(info.width), height = Int(info.height), stride = Int(info.stride)
        guard width > 0, width <= 512, height > 0, height <= 128 * ids.count,
            height % ids.count == 0, stride == width * 4, info.count == stride * height,
            let space = CGColorSpace(name: CGColorSpace.displayP3) else { return nil }
        let rowHeight = height / ids.count, rowBytes = stride * rowHeight
        var images: [String: CGImage] = [:]
        for (index, id) in ids.enumerated() {
            let data = Data(bytes: pixels.advanced(by: index * rowBytes), count: rowBytes)
            guard let provider = CGDataProvider(data: data as CFData),
                let image = CGImage(width: width, height: rowHeight, bitsPerComponent: 8, bitsPerPixel: 32,
                    bytesPerRow: stride, space: space,
                    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                    provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else { return nil }
            images[id] = image
        }
        return Decoded(request: info.request, images: images)
    }
}

/// Native geometry, wake-ups and image presentation. Rust owns the request
/// lifecycle, source generations, row batching, retry and cache limits.
@MainActor final class FilterPreviews: ObservableObject {
    @Published private(set) var images: [String: CGImage] = [:]
    private weak var store: EditorStore?
    private struct Visible { let id: String; let size: [Int] }
    private var visible: [String: Visible] = [:]
    private var key = ""
    private var task: Task<Void, Never>?
    private var generation: UInt64 = 0
    private var active = true
    private var publishedVisible = false
    init(store: EditorStore) { self.store = store }

    func reset() {
        generation &+= 1; task?.cancel(); task = nil
        images.removeAll(); key = ""; refresh()
    }
    func setActive(_ value: Bool) { active = value; refresh() }
    func show(token: String, id: String, width: CGFloat, scale: CGFloat) {
        visible[token] = Visible(id: id, size: [min(512, max(80, Int(width * scale))), min(128, max(1, Int(40 * scale)))])
        refresh()
    }
    func hide(_ token: String) { visible.removeValue(forKey: token); refresh() }
    func hidePanel(_ prefix: String) { visible = visible.filter { !$0.key.hasPrefix(prefix + ":") }; refresh() }
    func refresh() {
        guard task == nil, store?.native != nil,
            (active && !visible.isEmpty) || publishedVisible else { return }
        let generation = generation
        task = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                let hadVisible = self.active && !self.visible.isEmpty
                let wait = await self.poll()
                guard !Task.isCancelled, generation == self.generation else { return }
                if !self.active || self.visible.isEmpty {
                    if hadVisible { continue } // Deliver empty geometry before sleeping.
                    self.task = nil; return
                }
                // A zero shared delay means prompt bounded completion service.
                // The native timer yields; it never spins on the render owner.
                do { try await Task.sleep(for: .milliseconds(max(8, wait))) } catch { return }
            }
        }
    }
    private func poll() async -> Int {
        let generation = generation
        guard let store, let native = store.native else { return 200 }
        let rows = active ? Array(visible.values) : []
        let ids = Array(Set(rows.map(\.id))).sorted()
        let size = [rows.map { $0.size[0] }.max() ?? 80, rows.map { $0.size[1] }.max() ?? 40]
        let query = JSON(["type": "filter_previews", "filters": ids, "size": size,
            "cache": ["key": key, "rows": Array(images.keys)]])
        // A hidden view publishes empty geometry once, without creating a new
        // native query for every subsequent drawing snapshot.
        publishedVisible = !ids.isEmpty
        let reply: FilterPreviewReply = await withCheckedContinuation { continuation in
            native.filterPreviews(query) { continuation.resume(returning: $0) }
        }
        guard generation == self.generation else { return 200 }
        if let error = reply.error { store.failure = error; return 1000 }
        guard reply.status["epoch"].uint == store.state["document_file"]["epoch"].uint else { return 200 }
        if !reply.status["error"].isNull { store.failure = reply.status["error"].string }
        let next = reply.status["key"].string
        if key != next { key = next; images.removeAll() }
        let retained = Set(reply.status["retained"].array.map(\.string))
        images = images.filter { retained.contains($0.key) }
        if let atlas = reply.atlas {
            let decoded = await Task.detached(priority: .utility) { atlas.decode() }.value
            guard generation == self.generation,
                reply.status["epoch"].uint == store.state["document_file"]["epoch"].uint else { return 200 }
            if let decoded { images.merge(decoded.images) { _, image in image } }
        }
        return Int(reply.status["wait_ms"].number)
    }
}
