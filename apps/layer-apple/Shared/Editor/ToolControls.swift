import SwiftUI

/// Tool families, subtools and settings are live shared models, also used by
/// the desktop host. This includes non-paint tools and their command actions.
struct ToolSetControls: View {
    @ObservedObject var store: EditorStore
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        if store.state["tool_set"]["groups"].array.contains(where: { $0["action"]["type"].string == "select_tool_group" }) {
            // Keep every brush reachable until Apple toolbar customization can
            // expose all paint families. This is also the current web list.
            VStack(alignment: .leading, spacing: 2) {
                ForEach(store.catalog["brush_categories"].array.indices, id: \.self) { index in
                    let category = store.catalog["brush_categories"][index]
                    Text(category["label"].string).fontWeight(.bold).opacity(0.55).padding(8)
                    items(JSON(category["brushes"].array.map { brush in
                        ["label": brush["label"].raw, "preview": brush["id"].raw,
                         "selected": brush["id"].uint == store.state["brush"]["preset"].uint,
                         "action": ["type": "select_brush", "id": brush["id"].raw]]
                    }), group: false)
                }
            }
        } else {
            VStack(spacing: 6) {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 140), spacing: 2)], spacing: 2) {
                    items(store.state["tool_set"]["groups"], group: true)
                }
                VStack(spacing: 2) { items(store.state["tool_set"]["subtools"], group: false) }
            }
        }
    }

    private func items(_ items: JSON, group: Bool) -> some View {
        ForEach(items.array.indices, id: \.self) { index in
            let item = items[index]
            Button { store.dispatch(item["action"]) } label: {
                Group {
                    if !item["preview"].isNull {
                        VStack(alignment: .trailing, spacing: 0) {
                            Image("preview-\(item["preview"].uint)-\(store.state["theme"].string)").resizable().frame(height: 40)
                            Text(item["label"].string).fontWeight(.bold)
                        }.padding(.horizontal, 6).padding(.vertical, 3)
                    } else {
                        HStack(spacing: 6) {
                            SharedIcon(name: item["icon"].string)
                            Text(item["label"].string).fontWeight(.bold)
                        }.frame(minHeight: 36)
                    }
                }.frame(maxWidth: .infinity)
                    .background(item["selected"].bool ? palette.active : Color.clear, in: RoundedRectangle(cornerRadius: 6))
                    .contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel(item["label"].string)
                .accessibilityAddTraits(item["selected"].bool ? .isSelected : [])
                .accessibilityIdentifier(item["preview"].isNull ? "tool-\(group ? "group" : "subtool")-\(index)" : "brush-\(item["preview"].uint)")
        }
    }
}

struct ToolSettingsControls: View {
    @ObservedObject var store: EditorStore
    private var settings: [JSON] { store.state["tool_settings"].array }
    // Changing tool or target must discard an unfinished field draft. Ordinary
    // value updates retain view identity, focus and selection.
    private var context: String {
        let selected = ["groups", "subtools"].flatMap { store.state["tool_set"][$0].array }
            .filter { $0["selected"].bool }.map { $0["action"].stableKey }.joined(separator: ":")
        return selected + ":" + String(store.state["brush"]["preset"].uint)
            + ":" + String(store.state["layer_tools"]["editing_layer"]["id"].uint)
            + ":" + String(store.state["layer_tools"]["editing_layer"]["mask_selected"].bool)
    }
    var body: some View {
        let editingContext = context
        VStack(alignment: .leading, spacing: 6) {
            ForEach(settings, id: \.settingID) { item in
                let index = settings.firstIndex { $0.settingID == item.settingID } ?? 0
                if !item["group"].string.isEmpty && (index == 0 || settings[index - 1]["group"].string != item["group"].string) {
                    Text(item["group"].string).opacity(0.55).padding(.top, 6)
                }
                NumberControl(store: store, label: item["label"].string, value: item["value"].number,
                    control: item["numeric"], identifier: "tool-" + item.settingID) { value, completion in
                    guard editingContext == context else { completion(nil); return }
                    store.edit(["type": "set_tool_setting", "id": item.settingID, "value": value], completion: completion)
                }.id(context + item.settingID + item["label"].string + item["numeric"].stableKey)
            }
            ForEach(store.state["tool_actions"].array.indices, id: \.self) { index in
                let action = store.state["tool_actions"][index]
                let command = store.command(action["command"].string)
                if action["checkable"].bool {
                    Toggle(command["label"].string, isOn: Binding(get: { command["selected"].bool }, set: { _ in store.invoke(command["id"].string) }))
                        .disabled(!command["enabled"].bool).help(command["tooltip"].string)
                        .accessibilityIdentifier("tool-action-" + command["id"].string)
                } else {
                    Button(command["label"].string) { store.invoke(command["id"].string) }
                        .disabled(!command["enabled"].bool).help(command["tooltip"].string)
                        .accessibilityIdentifier("tool-action-" + command["id"].string)
                }
            }
        }
    }
}

private extension JSON {
    var settingID: String { self["id"].string }
    var stableKey: String {
        (try? JSONSerialization.data(withJSONObject: raw, options: [.fragmentsAllowed, .sortedKeys]))
            .map { String(decoding: $0, as: UTF8.self) } ?? ""
    }
}
