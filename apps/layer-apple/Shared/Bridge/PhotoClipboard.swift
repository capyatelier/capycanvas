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
    typealias Loader = (@escaping (Result<Content, Error>) -> Void) -> Void
    let representations: [Loader]
    var load: Loader { representations[0] }
    init(name: String = "Pasted image", load: @escaping (@escaping (Result<Content, Error>) -> Void) -> Void) {
        self.name = name; self.representations = [load]
    }
    init(fileURL: URL) {
        name = fileURL.lastPathComponent; representations = [{ $0(.success(.file(fileURL))) }]
    }
    init(name: String = "Pasted image", representations: [Loader]) { self.name = name; self.representations = representations }
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
        let available = ([UTType.fileURL] + types).filter { provider.hasItemConformingToTypeIdentifier($0.identifier) }
        guard !available.isEmpty else { return nil }
        return PhotoItem(name: provider.suggestedName ?? "Imported image", representations: available.map { type in
            { done in
                provider.loadDataRepresentation(forTypeIdentifier: type.identifier) { data, error in
                    DispatchQueue.main.async {
                        if let data { done(Result { try content(data, file: type == .fileURL) }) }
                        else { done(.failure(error ?? HostFailure(message: "Could not read an image"))) }
                    }
                }
            }
        })
    }
}

@MainActor enum PhotoClipboard {
    private static let nonceType = "art.capycanvas.clip.nonce"
    #if os(macOS)
    private final class PNGProvider: NSObject, NSPasteboardItemDataProvider {
        let png: Data
        init(_ png: Data) { self.png = png }
        func pasteboard(_ pasteboard: NSPasteboard?, item: NSPasteboardItem, provideDataForType type: NSPasteboard.PasteboardType) {
            item.setData(png, forType: type)
        }
    }
    private static var provider: PNGProvider?
    #endif
    static func write(png: Data, nonce: String, failure: String) throws {
        #if os(macOS)
        let item = NSPasteboardItem(), provider = PNGProvider(png)
        item.setDataProvider(provider, forTypes: [.png])
        item.setString(nonce, forType: NSPasteboard.PasteboardType(nonceType))
        NSPasteboard.general.clearContents()
        guard NSPasteboard.general.writeObjects([item]) else {
            throw HostFailure(message: failure)
        }
        self.provider = provider
        #else
        let item = NSItemProvider()
        item.registerDataRepresentation(forTypeIdentifier: UTType.png.identifier, visibility: .all) { $0(png, nil); return nil }
        item.registerDataRepresentation(forTypeIdentifier: nonceType, visibility: .all) { $0(Data(nonce.utf8), nil); return nil }
        UIPasteboard.general.setItemProviders([item], localOnly: false, expirationDate: nil)
        #endif
    }
    static var nonce: String? {
        #if os(macOS)
        NSPasteboard.general.string(forType: NSPasteboard.PasteboardType(nonceType))
        #else
        let board = UIPasteboard.general
        guard board.contains(pasteboardTypes: [nonceType]) else { return nil }
        return board.data(forPasteboardType: nonceType).flatMap { String(data: $0, encoding: .utf8) }
        #endif
    }
    static func read(_ completion: @escaping (Result<[PhotoItem], Error>) -> Void) {
        #if os(macOS)
        let types = UTType.capyPhotoTypes
        let images = (NSPasteboard.general.pasteboardItems ?? []).compactMap { item -> PhotoItem? in
            let available = ([UTType.fileURL] + types).map { NSPasteboard.PasteboardType($0.identifier) }.filter { item.types.contains($0) }
            guard !available.isEmpty else { return nil }
            return PhotoItem(representations: available.map { type in
                { done in
                    if let data = item.data(forType: type) {
                        done(Result { try PhotoItem.content(data, file: type.rawValue == UTType.fileURL.identifier) })
                    } else { done(.failure(HostFailure(message: "Could not read a clipboard image"))) }
                }
            })
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
