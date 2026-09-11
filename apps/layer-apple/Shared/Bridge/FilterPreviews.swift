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
            let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
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

/// One producer per editor, shared by every projection of the Filters panel.
/// Observe visible geometry and structural revisions; zoom and idle frames do
/// not rebuild images or poll. Keep the largest requested row until replacement.
@MainActor final class FilterPreviews: ObservableObject {
    @Published private(set) var images: [String: CGImage] = [:]
    private weak var store: EditorStore?
    private struct Visible { let id: String; let size: [Int] }
    private struct Key: Equatable {
        let epoch: UInt64
        let revision: [UInt64]
        let size: [Int]
    }
    private struct Pending { let request: UInt64; let key: Key }
    private var visible: [String: Visible] = [:]
    private var completed: [String: Key] = [:]
    private var key: Key?
    private var pending: Pending?
    private var next: UInt64 = 0
    private var size = [80, 40]
    private var task: Task<Void, Never>?
    init(store: EditorStore) { self.store = store }

    func show(token: String, id: String, width: CGFloat, scale: CGFloat) {
        let value = Visible(id: id, size: [min(512, max(80, Int(width * scale))), min(128, max(1, Int(40 * scale)))])
        visible[token] = value
        refresh()
    }
    func hide(_ token: String) { visible.removeValue(forKey: token) }
    func hidePanel(_ prefix: String) { visible = visible.filter { !$0.key.hasPrefix(prefix + ":") } }
    func refresh() {
        guard let store else { return }
        let epoch = store.state["document_file"]["epoch"].uint
        if let key, key.epoch != epoch {
            pending = nil; completed.removeAll(); images.removeAll(); size = [80, 40]
        }
        for value in visible.values { size = zip(size, value.size).map { max($0, $1) } }
        let revision = store.snapshot["filter_preview_revision"].array.map(\.uint)
        guard revision.count == 3 else { return }
        let newKey = Key(epoch: epoch, revision: revision, size: size)
        if key != newKey {
            key = newKey
            // Never show an old document/layer/catalog preview while replacing it.
            images.removeAll(); completed.removeAll()
        }
        guard task == nil, !visible.isEmpty,
            pending != nil || visible.values.contains(where: { completed[$0.id] != key }) else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                do { try await Task.sleep(for: .milliseconds(200)) } catch { return }
                guard let self else { return }
                let more = await self.poll()
                if !more { self.task = nil; return }
            }
        }
    }
    private func poll() async -> Bool {
        guard let store, let native = store.native, let key, !visible.isEmpty else { return false }
        let wanted = Set(visible.values.map(\.id))
        let missing = wanted.sorted().filter { completed[$0] != key }
        guard pending != nil || !missing.isEmpty else { return false }
        next &+= 1
        let request = next
        let query = JSON(["type": "filter_previews", "request": request, "revision": key.revision,
            "filters": pending == nil ? Array(missing.prefix(8)) : [], "size": key.size])
        let reply: FilterPreviewReply = await withCheckedContinuation { continuation in
            native.filterPreviews(query) { continuation.resume(returning: $0) }
        }
        // A New/Open may replace the renderer while this queue reply is in
        // flight. Its old request can never complete on the new renderer.
        guard key.epoch == self.key?.epoch else { return !visible.isEmpty }
        if let error = reply.error { pending = nil; store.failure = error; return false }
        if reply.status["accepted"].bool { pending = Pending(request: request, key: key) }
        if let atlas = reply.atlas {
            let decoded = await Task.detached(priority: .utility) { atlas.decode() }.value
            if let decoded, let job = pending, decoded.request == job.request {
                pending = nil
                if job.key == self.key, job.key.revision == reply.status["revision"].array.map(\.uint) {
                    var updated = images
                    for (id, image) in decoded.images { updated[id] = image; completed[id] = job.key }
                    images = updated
                }
            } else if decoded == nil { pending = nil; return false }
        }
        return !visible.isEmpty && (pending != nil || visible.values.contains { completed[$0.id] != self.key })
    }
}
