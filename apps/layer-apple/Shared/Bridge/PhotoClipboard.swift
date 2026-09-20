import Foundation
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// One deferred transport item, shared by picker, clipboard and external drops.
/// Files stay streamed/coordinated; encoded representations are never redrawn.
@MainActor struct PhotoItem {
    enum Content { case file(URL), image(Data) }
    let name: String
    let load: (@escaping (Result<Content, Error>) -> Void) -> Void
    init(name: String = "Pasted image", load: @escaping (@escaping (Result<Content, Error>) -> Void) -> Void) {
        self.name = name; self.load = load
    }
    init(fileURL: URL) {
        name = fileURL.lastPathComponent; load = { $0(.success(.file(fileURL))) }
    }
    static func content(_ data: Data, file: Bool) throws -> Content {
        if !file { return .image(data) }
        guard let url = URL(dataRepresentation: data, relativeTo: nil), url.isFileURL else {
            throw HostFailure(message: "The image provider did not supply a local file")
        }
        return .file(url)
    }
    static func provider(_ provider: NSItemProvider) -> PhotoItem? { makeProvider(provider, types: UTType.capyPhotoTypes) }
    static func drawingProvider(_ provider: NSItemProvider) -> PhotoItem? { makeProvider(provider, types: [.capyProject] + UTType.capyPhotoTypes) }
    private static func makeProvider(_ provider: NSItemProvider, types: [UTType]) -> PhotoItem? {
        let file = provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier)
        let type = file ? UTType.fileURL : types.first { provider.hasItemConformingToTypeIdentifier($0.identifier) }
        guard let type else { return nil }
        return PhotoItem(name: provider.suggestedName ?? "Imported image") { done in
            provider.loadDataRepresentation(forTypeIdentifier: type.identifier) { data, error in
                DispatchQueue.main.async {
                    if let data { done(Result { try content(data, file: file) }) }
                    else { done(.failure(error ?? HostFailure(message: "Could not read an image"))) }
                }
            }
        }
    }
}

@MainActor enum PhotoClipboard {
    static func read(_ completion: @escaping (Result<[PhotoItem], Error>) -> Void) {
        #if os(macOS)
        let types = UTType.capyPhotoTypes
        let images = (NSPasteboard.general.pasteboardItems ?? []).compactMap { item -> PhotoItem? in
            guard let type = ([UTType.fileURL] + types).map({ NSPasteboard.PasteboardType($0.identifier) })
                .first(where: { item.types.contains($0) }) else { return nil }
            return PhotoItem { done in
                if let data = item.data(forType: type) {
                    done(Result { try PhotoItem.content(data, file: type.rawValue == UTType.fileURL.identifier) })
                }
                else { done(.failure(HostFailure(message: "Could not read a clipboard image"))) }
            }
        }
        #else
        let images = UIPasteboard.general.itemProviders.compactMap(PhotoItem.provider)
        #endif
        // Decode each item before requesting the next encoded representation.
        // Shared batch limits can stop loading without retaining every input.
        if !images.isEmpty { completion(.success(images)); return }
        completion(.failure(HostFailure(message: "Copy a supported image to paste.")))
    }
}
