import Foundation

/// Immutable transport values. Rust remains authoritative for models and actions.
struct JSON: @unchecked Sendable {
    let raw: Any
    init(_ raw: Any = NSNull()) { self.raw = raw }
    subscript(_ key: String) -> JSON { JSON((raw as? [String: Any])?[key] ?? NSNull()) }
    subscript(_ index: Int) -> JSON {
        let values = raw as? [Any] ?? []
        return values.indices.contains(index) ? JSON(values[index]) : JSON()
    }
    var array: [JSON] { (raw as? [Any] ?? []).map(JSON.init) }
    var object: [String: Any] { raw as? [String: Any] ?? [:] }
    var string: String { raw as? String ?? "" }
    var number: Double { (raw as? NSNumber)?.doubleValue ?? 0 }
    var uint: UInt64 { (raw as? NSNumber)?.uint64Value ?? 0 }
    var bool: Bool { (raw as? NSNumber)?.boolValue ?? false }
    var isNull: Bool { raw is NSNull }
    func replacing(_ key: String, with value: JSON) -> JSON {
        var result = object; result[key] = value.raw; return JSON(result)
    }
    func encoded() throws -> String {
        String(decoding: try JSONSerialization.data(withJSONObject: raw, options: [.fragmentsAllowed]), as: UTF8.self)
    }
    var stableKey: String {
        (try? JSONSerialization.data(withJSONObject: raw, options: [.fragmentsAllowed, .sortedKeys]))
            .map { String(decoding: $0, as: UTF8.self) } ?? ""
    }
    static func decode(_ text: String) throws -> JSON {
        JSON(try JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed]))
    }
}

struct HostFailure: Error, LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
