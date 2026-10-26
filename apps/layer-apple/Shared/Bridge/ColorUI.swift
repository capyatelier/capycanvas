import Foundation

/// The OS owns destination-profile conversion. These are observations of the
/// window's current screen, never document or export preferences.
struct DisplayDetails: Equatable {
    var screen = ""
    var destination = ""
}

/// Shared parsing and color transforms, independent of the renderer owner.
enum ColorUI {
    static func resolve(_ request: [String: Any], language: String = "en") -> JSON {
        do {
            let text = try JSON(["language": language, "request": request]).encoded()
            guard let response = text.withCString({ takeNativeText(capy_apple_color_ui($0)) }) else {
                throw HostFailure(message: "Color input failed")
            }

            return try JSON.decode(response)
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    static func profileEntries(_ entries: [JSON], language: String, query: String = "") -> [JSON] {
        let rows = resolve(["type": "profile_entries", "entries": entries.map(\.raw), "query": query], language: language).array
        return rows.map { row in row.replacing("visible", with: entries.first { $0["id"].string == row["id"].string }?["visible"] ?? JSON(false)) }
            .sorted { $0["name"].string.localizedStandardCompare($1["name"].string) == .orderedAscending }
    }
    static func preview(_ color: JSON) -> JSON {
        resolve(["type": "preview", "colors": [color.raw], "display_space": "DisplayP3"])[0]
    }
}

extension Double {
    var clampedHeadroom: Double { isFinite ? min(100, max(1, self)) : 1 }
}
