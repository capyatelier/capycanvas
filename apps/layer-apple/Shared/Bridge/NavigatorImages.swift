import SwiftUI
import CoreGraphics

/// Independent immutable pixels, never a lease on the live editor or renderer.
final class NativePreviewImage: @unchecked Sendable {
    private let handle: OpaquePointer
    init(_ handle: OpaquePointer) { self.handle = handle }
    deinit { capy_preview_image_free(handle) }
    struct Bitmap: @unchecked Sendable { let epoch: UInt64; let image: CGImage }
    func decode() -> Bitmap? {
        var info = CapyPreviewImageInfo()
        capy_preview_image_read(handle, &info)
        let width = Int(info.width), height = Int(info.height), stride = Int(info.stride)
        guard let pixels = info.pixels, width > 0, height > 0, width <= 256, height <= 256,
            stride == width * 4, info.count == stride * height else { return nil }
        let data = Data(bytes: pixels, count: info.count)
        guard let provider = CGDataProvider(data: data as CFData), let space = CGColorSpace(name: CGColorSpace.sRGB),
            let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: stride, space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else { return nil }
        return Bitmap(epoch: info.key.epoch, image: image)
    }
}

@MainActor final class NavigatorImages: ObservableObject {
    @Published private(set) var image: CGImage?
    private weak var store: EditorStore?
    private var viewers: Set<UUID> = []
    private var epoch: UInt64?
    init(store: EditorStore) { self.store = store }
    func refresh() {
        let next = store?.state["document_file"]["epoch"].uint
        if next != epoch { epoch = next; image = nil }
    }
    func show(_ id: UUID) {
        let wasEmpty = viewers.isEmpty
        viewers.insert(id); refresh()
        guard wasEmpty else { return }
        store?.native?.setNavigatorReceiver { [weak self] source, complete in
            Task { @MainActor [weak self] in
                let decoded = await Task.detached(priority: .utility) { source.decode() }.value
                if let self, let decoded, decoded.epoch == self.epoch { self.image = decoded.image }
                complete()
            }
        }
    }
    func hide(_ id: UUID) {
        viewers.remove(id)
        if viewers.isEmpty { store?.native?.setNavigatorReceiver(nil) }
    }
}

@MainActor final class RendererStats: ObservableObject {
    @Published private(set) var view = JSON()
    private weak var store: EditorStore?
    private var viewers: Set<UUID> = []
    private var task: Task<Void, Never>?
    init(store: EditorStore) { self.store = store }
    func show(_ id: UUID) {
        viewers.insert(id)
        guard task == nil else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                guard let store = self?.store else { return }
                let next: JSON = await withCheckedContinuation { continuation in
                    store.query(["type": "renderer_stats"]) { continuation.resume(returning: $0) }
                }
                if Task.isCancelled { return }
                if let self, self.view.stableKey != next.stableKey { self.view = next }
                do { try await Task.sleep(for: .milliseconds(200)) } catch { return }
            }
        }
    }
    func hide(_ id: UUID) {
        viewers.remove(id)
        if viewers.isEmpty { task?.cancel(); task = nil }
    }
}
