import Foundation

enum ToolbarUI {
    static func resolve(_ request: [String: Any]) -> JSON {
        do {
            let text = try JSON(request).encoded()
            guard let response = text.withCString({ capy_apple_toolbar_ui($0) }) else {
                throw HostFailure(message: "Toolbar request failed")
            }
            defer { capy_apple_string_free(response) }
            return try JSON.decode(String(cString: response))
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    @MainActor private static var cache: [String: JSON] = [:]
    @MainActor static func cached(_ request: [String: Any]) -> JSON {
        let key = JSON(request).stableKey
        if let value = cache[key] { return value }
        if cache.count > 512 { cache.removeAll() }
        let value = resolve(request)
        cache[key] = value
        return value
    }
    static func formatted(_ control: JSON, value: Double, units: Bool = true) -> JSON {
        resolve(["type": "number", "request": ["control": control.raw, "value": value, "operation": ["type": "format"]],
            "compact": true, "units": units])
    }
}
