import Foundation
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Read the encoded representation only when Paste is invoked. Native image
/// drawing/re-encoding would discard the source profile or integer precision.
@MainActor enum PhotoClipboard {
    struct Item {
        let load: (@escaping (Result<Data, Error>) -> Void) -> Void
    }
    static func read(_ completion: @escaping (Result<[Item], Error>) -> Void) {
        let types = UTType.capyPhotoTypes
        #if os(macOS)
        let images = (NSPasteboard.general.pasteboardItems ?? []).compactMap { item -> Item? in
            guard let type = types.map({ NSPasteboard.PasteboardType($0.identifier) })
                .first(where: { item.types.contains($0) }) else { return nil }
            return Item { done in
                if let data = item.data(forType: type) { done(.success(data)) }
                else { done(.failure(HostFailure(message: "Could not read a clipboard image"))) }
            }
        }
        #else
        let images = UIPasteboard.general.itemProviders.compactMap { provider -> Item? in
            guard let type = types.first(where: { provider.hasItemConformingToTypeIdentifier($0.identifier) })
                else { return nil }
            return Item { done in
                provider.loadDataRepresentation(forTypeIdentifier: type.identifier) { data, error in
                    DispatchQueue.main.async {
                        if let data { done(.success(data)) }
                        else { done(.failure(error ?? HostFailure(message: "Could not read a clipboard image"))) }
                    }
                }
            }
        }
        #endif
        // Decode each item before requesting the next encoded representation.
        // Shared batch limits can stop loading without retaining every input.
        if !images.isEmpty { completion(.success(images)); return }
        completion(.failure(HostFailure(message: "Copy a supported image to paste.")))
    }
}
