import SwiftUI
import CoreGraphics

final class NativeBrushStamp: @unchecked Sendable {
    private let handle: UInt64
    let metadata: JSON
    let count: Int
    init?(_ handle: UInt64) {
        var info = CapyBrushStampInfo()
        guard capy_brush_stamp_read(handle, &info) else { capy_brush_stamp_free(handle); return nil }
        self.handle = handle
        metadata = JSON(["request_id": info.request, "pending": info.pending, "width": info.width, "height": info.height, "extent": Double(info.extent)])
        count = info.count
    }
    deinit { capy_brush_stamp_free(handle) }
    func image() -> CGImage? {
        let width = Int(metadata["width"].uint), height = Int(metadata["height"].uint)
        guard width > 0, height > 0, count == width * height,
            let alpha = copyNativeData(count, copy: { capy_brush_stamp_copy(handle, $0, $1, $2) }) else { return nil }
        let bytes = alpha.flatMap { [0, 0, 0, $0] as [UInt8] }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }
}

struct FilterPreviewReply: Sendable {
    let status: JSON
    let atlas: NativeFilterPreviews?
    let error: String?
}

/// An immutable Rust allocation, independent of the live editor. Image creation
/// happens on a utility worker; the render owner only transfers its ownership.
final class NativeFilterPreviews: @unchecked Sendable {
    private let handle: UInt64
    init(_ handle: UInt64) { self.handle = handle }
    deinit { capy_filter_previews_free(nativeValue(handle)) }
    struct Decoded: @unchecked Sendable {
        let request: UInt64
        let images: [String: CGImage]
    }
    func decode() -> Decoded? {
        var info = CapyFilterPreviewInfo()
        guard capy_filter_previews_read(nativeValue(handle), &info), let filters = takeNativeText(info.filters),
            let ids = try? JSON.decode(filters).array.map(\.string),
            !ids.isEmpty, ids.count <= 8 else { return nil }
        let width = Int(info.width), height = Int(info.height), stride = Int(info.stride)
        guard width > 0, width <= 512, height > 0, height <= 128 * ids.count,
            height % ids.count == 0, stride == width * 4, info.count == stride * height,
            let space = CGColorSpace(name: CGColorSpace.displayP3) else { return nil }
        let rowHeight = height / ids.count, rowBytes = stride * rowHeight
        var images: [String: CGImage] = [:]
        for (index, id) in ids.enumerated() {
            guard let data = copyNativeData(rowBytes, copy: { capy_filter_previews_copy(handle, index * rowBytes + $0, $1, $2) }),
                let provider = CGDataProvider(data: data as CFData),
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
    private var visible: [String: String] = [:]
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
    func show(token: String, id: String) {
        visible[token] = id
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
        let ids = active ? Array(Set(visible.values)).sorted() : []
        let query = JSON(["type": "filter_previews", "filters": ids,
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
