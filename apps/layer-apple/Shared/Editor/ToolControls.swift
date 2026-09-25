import SwiftUI

/// Tool families, subtools and settings are live shared models, also used by
/// the desktop host. This includes non-paint tools and their command actions.
struct ToolSetControls: View {
    @ObservedObject var store: EditorStore
    var panel = "brushes"
    private var model: JSON { panel == "brushes" ? store.state["tool_set"] : store.state["tool_panels"][panel] }
    private var sets: Bool { panel == "brush_sets" || panel == "sculpt_sets" }
    var body: some View {
        VStack(spacing: 6) {
            if sets {
                VStack(spacing: 2) { items(model["groups"], group: true) }
            } else {
                ToolGroupsLayout { items(model["groups"], group: true) }
            }
            VStack(spacing: 2) { items(model["subtools"], group: false) }
        }
    }

    private func items(_ items: JSON, group: Bool) -> some View {
        ForEach(items.array.indices, id: \.self) { index in
            let item = items[index]
            let command = item["action"]["type"].string == "invoke"
                ? store.command(item["action"]["command"].string) : JSON()
            Button { store.dispatch(item["action"]) } label: {
                Group {
                    if group && sets {
                        HStack(spacing: 8) {
                            SharedIcon(name: item["icon"].string)
                            Text(item["label"].string).fontWeight(.bold).lineLimit(1)
                            Spacer(minLength: 0)
                        }.padding(.horizontal, 6).frame(minHeight: 44)
                    } else if group {
                        HStack(spacing: 6) {
                            SharedIcon(name: item["icon"].string)
                            Text(item["label"].string).fontWeight(.bold).lineLimit(1).truncationMode(.tail)
                                .frame(maxWidth: .infinity, alignment: .trailing)
                        }.padding(.horizontal, 6).frame(maxHeight: .infinity)
                    } else {
                        VStack(spacing: 0) {
                            if !item["preview"].isNull {
                                Image("preview-\(item["preview"].uint)-\(store.state["theme"].string)")
                                    .resizable().frame(height: 40)
                                    .clipShape(RoundedRectangle(cornerRadius: 3))
                            }
                            HStack(spacing: 6) {
                                SharedIcon(name: item["icon"].string)
                                Text(item["label"].string).fontWeight(.bold)
                                    .lineLimit(1).truncationMode(.tail)
                                    .frame(maxWidth: .infinity, minHeight: item["preview"].isNull ? store.catalog["text_size_pt"].number * 4 / 3 * 1.66 : 18,
                                        alignment: item["preview"].isNull ? .leading : .trailing)
                            }
                        }
                    }
                }.padding(.horizontal, group ? 0 : 6).padding(.vertical, group ? 0 : 3)
                    .frame(maxWidth: .infinity, minHeight: group ? nil : item["preview"].isNull ? (store.tonalActive ? 36 : 44) : 64)
                    .contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle(selected: item["selected"].bool))
                .disabled(!command.isNull && !command["enabled"].bool)
                .opacity(!command.isNull && !command["enabled"].bool ? 0.36 : 1)
                .help(command.isNull ? item["label"].string : command["tooltip"].string)
                .accessibilityLabel(item["label"].string)
                .accessibilityAddTraits(item["selected"].bool ? .isSelected : [])
                .accessibilityIdentifier(sets ? "\(panel)-\(item["label"].string)" : item["preview"].isNull ? "tool-\(group ? "group" : "subtool")-\(index)" : "brush-\(item["preview"].uint)")
        }
    }
}

/// Group buttons are three toolbar tiles wide and one tall, wrapping with
/// the toolbar tile gap, as GTK's tool panels and the web lay them out.
private struct ToolGroupsLayout: Layout {
    private static let tile: CGFloat = 36, gap: CGFloat = 2
    private func item(_ width: CGFloat) -> (width: CGFloat, columns: Int) {
        let natural = 3 * Self.tile + 2 * Self.gap
        return (min(natural, width), max(1, Int((width + Self.gap) / (natural + Self.gap))))
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let requested = proposal.width ?? 226
        let width = requested.isFinite ? max(0, requested) : 226
        let rows = (subviews.count + item(width).columns - 1) / item(width).columns
        return CGSize(width: width, height: CGFloat(rows) * Self.tile + CGFloat(max(0, rows - 1)) * Self.gap)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let (width, columns) = item(bounds.width)
        for index in subviews.indices {
            subviews[index].place(at: CGPoint(x: bounds.minX + CGFloat(index % columns) * (width + Self.gap),
                y: bounds.minY + CGFloat(index / columns) * (Self.tile + Self.gap)),
                anchor: .topLeading, proposal: ProposedViewSize(width: width, height: Self.tile))
        }
    }
}

struct ToolSettingsControls: View {
    @ObservedObject var store: EditorStore
    private var settings: [JSON] { store.state["tool_settings"].array }
    // Changing document, tool or target must discard an unfinished field draft.
    // Layer IDs can be reused by a new document. Ordinary value updates retain
    // view identity, focus and selection.
    private var context: String {
        let selected = ["groups", "subtools"].flatMap { store.state["tool_set"][$0].array }
            .filter { $0["selected"].bool }.map { $0["action"].stableKey }.joined(separator: ":")
        return String(store.state["document_file"]["epoch"].uint)
            + ":" + String(store.state["toolbar_context_generation"].uint)
            + ":" + selected + ":" + String(store.state["brush"]["preset"].uint)
            + ":" + String(store.state["layer_tools"]["editing_layer"]["id"].uint)
            + ":" + String(store.state["layer_tools"]["editing_layer"]["mask_selected"].bool)
    }
    var body: some View {
        if ["pick_visible", "pick_layer"].contains(store.state["layer_tools"]["tool"].string) {
            PickerSettingsRows(store: store)
        } else if store.tonalActive {
            TonalSettingsControls(store: store, context: context).id(context)
        } else {
            settingsBody
        }
    }
    @ViewBuilder private var settingsBody: some View {
        let editingContext = context
        let actions = store.state["tool_actions"].array
        let modes = actions.filter { SelectionModes.commands.contains($0["command"].string) }
        VStack(alignment: .leading, spacing: 8) {
            if !modes.isEmpty { SelectionModeGroup(store: store, actions: modes) }
            ForEach(settings, id: \.settingID) { item in
                let index = settings.firstIndex { $0.settingID == item.settingID } ?? 0
                if !item["group"].string.isEmpty && (index == 0 || settings[index - 1]["group"].string != item["group"].string) {
                    Text(item["group"].string).fontWeight(.bold).padding(.vertical, 4)
                }
                NumberControl(store: store, label: item["label"].string, value: item["value"].number,
                    control: item["numeric"], identifier: "tool-" + item.settingID) { value, completion in
                    guard editingContext == context else { completion(nil); return }
                    store.edit(["type": "set_tool_setting", "id": item.settingID, "value": value], completion: completion)
                }.id(context + item.settingID + item["label"].string + item["numeric"].stableKey)
            }
            ForEach(actions.indices, id: \.self) { index in
                let action = actions[index]
                if !SelectionModes.commands.contains(action["command"].string) {
                    let command = store.command(action["command"].string)
                    ToolActionControl(command: command, checkable: action["checkable"].bool,
                        textSize: store.catalog["text_size_pt"].number * 4 / 3) {
                        store.invoke(command["id"].string)
                    }
                }
            }
            if !modes.isEmpty { SelectionMenuButton(store: store, label: "Selection Actions…", kind: "selection") }
        }
    }
}

private extension JSON {
    var settingID: String { self["id"].string }
}

extension EditorStore {
    var tonalActive: Bool { state["tool_extra"].array.contains { $0["Choice"]["id"].string == "tonal-tones" } }
}

private struct TonalSettingsControls: View {
    @ObservedObject var store: EditorStore
    let context: String
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        let settings = store.state["tool_settings"].array
        let modes = store.state["tool_actions"].array.filter { SelectionModes.commands.contains($0["command"].string) }
        let bounds = settings.filter { ["tonal_lower", "tonal_upper"].contains($0["id"].string) }
        VStack(alignment: .leading, spacing: 2) {
            if !modes.isEmpty { SelectionModeGroup(store: store, actions: modes, height: 36) }
            ForEach(store.state["tool_extra"].array.indices, id: \.self) { index in
                let choice = store.state["tool_extra"][index]["Choice"]
                if !choice.isNull {
                    SegmentedChoiceBar(choice: choice, prefix: "tool", height: 36, iconSize: 20, palette: palette) { item in
                        store.dispatch(item["action"])
                    }
                }
            }
            if bounds.count == 2 {
                RangeControl(store: store, bounds: bounds, label: "Range in stops relative to reference white (0)",
                    prefix: "tool") { index, value, completion in
                    store.edit(["type": "set_tool_setting", "id": bounds[index]["id"].raw, "value": value], completion: completion)
                }
            }
            ForEach(settings.filter { !["tonal_lower", "tonal_upper"].contains($0["id"].string) }, id: \.settingID) { item in
                HStack(spacing: 6) {
                    Text(item["label"].string).lineLimit(1).frame(width: 62, alignment: .leading)
                    NumberControl(store: store, label: item["label"].string, value: item["value"].number,
                        control: item["numeric"], identifier: "tool-" + item.settingID, inline: true) { value, completion in
                        store.edit(["type": "set_tool_setting", "id": item.settingID, "value": value], completion: completion)
                    }
                }.frame(height: 28).accessibilityElement(children: .contain).accessibilityIdentifier("tool-setting-" + item.settingID)
            }
        }
    }
}

private struct PickerSettingsRows: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        let picker = store.state["color_picker"]
        VStack(alignment: .leading, spacing: 8) {
            row("Source") {
                Picker("Source", selection: Binding(get: { picker["layer"].bool }, set: { layer in
                    store.dispatch(["type": "color_picker", "action": ["kind": "source", "layer": layer]])
                })) {
                    Text("Visible color").tag(false)
                    if picker["can_sample_layer"].bool { Text("Selected layer").tag(true) }
                }.accessibilityIdentifier("picker-setting-source")
            }
            row("Sample size") {
                Picker("Sample size", selection: Binding(get: { picker["sample_width"].uint }, set: { width in
                    store.dispatch(["type": "set_color_sample_size", "width": width])
                })) {
                    ForEach(picker["sample_sizes"].array.map(\.uint), id: \.self) { width in
                        Text(width == 1 ? "Single pixel" : "\(width) px circle").tag(width)
                    }
                }.accessibilityIdentifier("picker-setting-size")
            }
        }.font(.system(size: 13))
    }
    private func row(_ label: String, @ViewBuilder control: () -> some View) -> some View {
        HStack(spacing: 8) {
            Text(label).lineLimit(1).fixedSize().frame(minWidth: 76, alignment: .leading)
            control().pickerStyle(.menu).labelsHidden().frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
