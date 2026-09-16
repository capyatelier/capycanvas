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
    static func read(_ completion: @escaping (Result<Data, Error>) -> Void) {
        let types: [UTType] = [.tiff, .png, .jpeg]
        #if os(macOS)
        let board = NSPasteboard.general
        for type in types {
            if let data = board.data(forType: NSPasteboard.PasteboardType(type.identifier)) {
                completion(.success(data)); return
            }
        }
        #else
        for provider in UIPasteboard.general.itemProviders {
            if let type = types.first(where: { provider.hasItemConformingToTypeIdentifier($0.identifier) }) {
                provider.loadDataRepresentation(forTypeIdentifier: type.identifier) { data, error in
                    let result: Result<Data, Error> = data.map { .success($0) }
                        ?? .failure(error ?? HostFailure(message: "Could not read the clipboard image"))
                    DispatchQueue.main.async { completion(result) }
                }
                return
            }
        }
        #endif
        completion(.failure(HostFailure(message: "Copy a PNG, JPEG or TIFF image to paste.")))
    }
}
