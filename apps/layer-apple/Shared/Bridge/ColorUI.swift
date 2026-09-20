import Foundation

/// The OS owns destination-profile conversion. These are observations of the
/// window's current screen, never document or export preferences.
struct DisplayDetails: Equatable {
    var screen = "Waiting for canvas"
    var destination = "Unavailable"
}

/// Shared parsing and color transforms, independent of the renderer owner.
enum ColorUI {
    static func resolve(_ request: [String: Any]) -> JSON {
        do {
            let text = try JSON(request).encoded()
            guard let response = text.withCString({ capy_apple_color_ui($0) }) else {
                throw HostFailure(message: "Color input failed")
            }
            defer { capy_apple_string_free(response) }
            return try JSON.decode(String(cString: response))
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    static func preview(_ color: JSON) -> JSON {
        resolve(["type": "preview", "colors": [color.raw], "display_space": "DisplayP3"])[0]
    }
}

extension Double {
    var clampedHeadroom: Double { isFinite ? min(100, max(1, self)) : 1 }
}
