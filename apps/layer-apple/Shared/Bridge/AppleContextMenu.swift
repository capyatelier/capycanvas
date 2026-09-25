import Foundation

/// Shared menu projection only. Rust owns item availability and action payloads;
/// AppKit/UIKit own presentation, tracking, accessibility and menu navigation.
@MainActor struct AppleContextMenu {
    let title: String
    let sections: [[Item]]
    struct Item {
        let label: String
        let identifier: String
        let enabled: Bool
        let selected: Bool?
        let hint: String
        let bindings: [JSON]
        let sections: [[Item]]
        let action: (() -> Void)?
    }
    var actions: [Item] {
        func leaves(_ sections: [[Item]]) -> [Item] {
            sections.flatMap { $0.flatMap { $0.sections.isEmpty ? [$0] : leaves($0.sections) } }
        }
        return leaves(sections)
    }
    init(_ model: JSON, invoke: @escaping (JSON) -> Void, command: ((JSON) -> Void)? = nil) {
        title = model["title"].string
        func decodeSections(_ value: JSON, parentEnabled: Bool = true) -> [[Item]] {
            value.array.map { section in
                section.array.map { item in
                    let payload = item["action"], request = item["command"]
                    let enabled = parentEnabled && item["enabled"].bool
                    let identifier = !item["identifier"].isNull ? item["identifier"].string
                        : !request.isNull ? (["palette-command", request["command"].string, request["format"].string, request["action"]["op"].string]
                            .filter { !$0.isEmpty }.joined(separator: "-"))
                        : payload["type"].string == "invoke" ? "command-" + payload["command"].string : "menu-action-" + item["label"].string
                    let action: (() -> Void)? = !request.isNull && command != nil ? { if enabled { command?(request) } }
                        : payload.isNull ? nil : { if enabled { invoke(payload) } }
                    return Item(label: item["label"].string, identifier: identifier, enabled: enabled,
                        selected: item["selected"].isNull ? nil : item["selected"].bool,
                        hint: item["hint"].string, bindings: item["bindings"].array,
                        sections: decodeSections(item["sections"], parentEnabled: enabled),
                        action: action)
                }
            }.filter { !$0.isEmpty }
        }
        sections = decodeSections(model["sections"])
    }
}
