import Foundation

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
        resolve(["type": "preview", "colors": [color.raw], "display_space": "Srgb"])[0]
    }
}
