import SwiftUI

/// Tool families, subtools and settings are live shared models, also used by
/// the desktop host. This includes non-paint tools and their command actions.
struct ToolSetControls: View {
    @ObservedObject var store: EditorStore
    var panel = "brushes"
    var drawerToolSet = JSON()
    private var model: JSON {
        drawerToolSet.isNull ? panel == "brushes" ? store.state["tool_set"] : store.state["tool_panels"][panel] : drawerToolSet
    }
    private var sets: Bool { panel == "brush_sets" || panel == "sculpt_sets" }
    var body: some View {
        VStack(spacing: 6) {
            if sets {
                VStack(spacing: 2) { items(model["groups"], group: true) }
            } else {
                ToolGroupsLayout { items(model["groups"], group: true) }
            }
            if !model["groups"].array.isEmpty && !model["subtools"].array.isEmpty {
                Rectangle().fill(EditorPalette(source: store.state["palette"])["text"].opacity(0.3)).frame(height: 1).padding(.horizontal, 4)
            }
            VStack(spacing: 2) { items(model["subtools"], group: false) }
        }
    }

    private func items(_ items: JSON, group: Bool) -> some View {
        ForEach(items.array.indices, id: \.self) { index in
            let item = items[index]
            let command = item["action"]["type"].string == "invoke"
                ? store.command(item["action"]["command"].string) : JSON()
            let enabled = item["enabled"].bool && (command.isNull || command["enabled"].bool)
            let reason = item["disabled_reason"].isNull ? command.disabledReason : item["disabled_reason"].string
            let identifier = sets ? "\(panel)-\(item["label"].string)" : item["preview"].isNull ? "tool-\(group ? "group" : "subtool")-\(index)" : "brush-\(item["preview"].uint)"
            Button { if !store.bubbles.input.consumeClick() { store.dispatch(item["action"]) } } label: {
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
                                BundledPreview(name: "preview-\(item["preview"].uint)-\(store.state["theme"].string)")
                                    .frame(height: 40)
                                    .clipShape(RoundedRectangle(cornerRadius: 3))
                            }
                            HStack(spacing: 6) {
                                SharedIcon(name: item["icon"].string)
                                Text(item["label"].string).fontWeight(.bold)
                                    .lineLimit(2).truncationMode(.tail)
                                    .frame(maxWidth: .infinity, minHeight: item["preview"].isNull ? store.catalog["text_size_pt"].number * 4 / 3 * 1.66 : 18,
                                        alignment: item["action"]["type"].string == "color_picker" ? .leading : .trailing)
                            }
                        }
                    }
                }.padding(.horizontal, group ? 0 : 6).padding(.vertical, group ? 0 : 3)
                    .frame(maxWidth: .infinity, minHeight: group ? nil : item["preview"].isNull ? (store.tonalActive ? 36 : 44) : 64)
                    .opacity(enabled && reason != nil ? 0.36 : 1)
                    .contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle(selected: item["selected"].bool))
                .modifier(BubbleChoiceSource(action: item["action"]))
                .disabled(!enabled).opacity(enabled ? 1 : 0.36)
                .modifier(ActionHelp(store: store, label: item["label"].string, action: item["action"], tooltip: reason ?? command["tooltip"].string))
                .accessibilityLabel(item["label"].string)
                .accessibilityAddTraits(item["selected"].bool ? .isSelected : [])
                .accessibilityIdentifier(identifier)
                .modifier(DisabledExplanation(reason: enabled ? nil : reason, identifier: identifier + "-reason"))
        }
    }
}

struct ActionHelp: ViewModifier {
    let store: EditorStore
    let label: String
    let action: JSON
    var tooltip = ""
    @State private var resolved: (request: String, text: String)?
    func body(content: Content) -> some View {
        let request = label + "\n" + action.stableKey
        content.help(!tooltip.isEmpty ? tooltip : resolved?.request == request ? resolved?.text ?? label : label)
            .onHover { inside in
                guard inside, tooltip.isEmpty else { return }
                store.query(["type": "action_tooltip", "label": label, "action": action.raw]) { reply in
                    if !reply.string.isEmpty { resolved = (request, reply.string) }
                }
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
    var body: some View { content.environment(\.compactPanelControls, true) }
    @ViewBuilder private var content: some View {
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
        let modes = actions.filter { $0["segmented"].bool }
        VStack(alignment: .leading, spacing: store.panelMetrics.group) {
            if !modes.isEmpty {
                VStack(spacing: store.panelMetrics.spacing) {
                    ForEach(actionGroups(modes), id: \.id) { group in
                        SelectionModeGroup(store: store, actions: group.items, height: store.panelMetrics.height, iconSize: store.panelMetrics.icon)
                    }
                    if modes.contains(where: { $0["group"].string == "selection-mode" }) {
                        SelectionMenuButton(store: store, label: store.catalog["native_copy"]["tool_controls"]["selection_menu"].string, kind: "selection", compact: true)
                    }
                }
            }
            VStack(alignment: .leading, spacing: store.panelMetrics.spacing) {
            ForEach(store.state["tool_extra"].array.map { $0["Gradient"] }.filter { !$0.isNull }, id: \.["gradient"]["destination"].stableKey) { gradient in
                GradientEditor(store: store, control: gradient, identifier: "tool-gradient") { action, completion in
                    store.edit(action as? [String: Any] ?? [:], completion: completion)
                }
            }
            let choices = store.state["tool_extra"].array.map { $0["Choice"] }.filter { !$0.isNull }
            ForEach(choices.filter { $0["beside"].isNull }, id: \.stableKey) { choice in
                ToolOptionField(store: store, option: JSON(["Choice": choice.raw]), iconSize: store.panelMetrics.icon, vertical: false, labeled: true,
                    style: "small", preferences: JSON(), stacked: false, prefix: "tool") { action, completion in
                    store.edit(action as? [String: Any] ?? [:], completion: completion)
                }.frame(height: choice["labeled"].bool ? nil : store.panelMetrics.height)
            }
            ForEach(settingGroups.map { ($0[0].settingID, $0) }, id: \.0) { _, group in
                let first = group[0], choice = choices.first { $0["beside"].string == first.settingID }
                let title = choice?["label"].string ?? first["group"].string
                if !title.isEmpty { Text(title).fontWeight(.bold).padding(.top, group[0].settingID == settings.first?.settingID ? 0 : 2) }
                if let choice {
                    HStack(spacing: store.panelMetrics.compound) {
                        SegmentedChoiceBar(choice: choice, prefix: "tool", height: nil, iconSize: 6, palette: palette) { store.dispatch($0["action"]) }
                        VStack(spacing: 2) {
                            ForEach(group, id: \.settingID) { item in
                                HStack(spacing: store.panelMetrics.spacing) {
                                    Text(item["label"].string).lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
                                    setting(item, editingContext: editingContext, valueOnly: true)
                                }.frame(minHeight: store.panelMetrics.height)
                            }
                        }
                    }
                } else {
                    ForEach(group, id: \.settingID) { setting($0, editingContext: editingContext) }
                }
            }
            ForEach(actionGroups(actions.filter { !$0["segmented"].bool }), id: \.id) { group in
                if !group.items[0]["group"].isNull { actionChoice(group) }
                else if !group.items[0]["row"].isNull {
                    HStack(spacing: store.panelMetrics.spacing) { ForEach(group.items, id: \.["command"].string) { actionButton($0) } }
                } else { actionButton(group.items[0]) }
            }
            }

        }
    }
}

private extension ToolSettingsControls {
    func actionGroups(_ actions: [JSON]) -> [(id: String, items: [JSON])] {
        actions.reduce(into: []) { groups, action in
            let id = action["group"].isNull ? action["row"].isNull ? action["command"].string : action["row"].string : action["group"].string
            if let index = groups.firstIndex(where: { $0.id == id }) { groups[index].items.append(action) }
            else { groups.append((id, [action])) }
        }
    }
    func actionButton(_ action: JSON) -> some View {
        let command = store.command(action["command"].string)
        let caption = store.state["canvas_bar"]["completion"].array.first { $0["option"]["Action"]["state"]["id"].string == command["id"].string }?["label"].string
        return ToolActionControl(command: command, checkable: action["checkable"].bool, side: store.panelMetrics.height,
            textSize: store.catalog["text_size_pt"].number * 4 / 3, caption: caption, iconOnly: action["icon_only"].bool) { store.invoke(command["id"].string) }
    }
    func actionChoice(_ group: (id: String, items: [JSON])) -> some View {
        let label = store.catalog["tool_action_groups"].array.first { $0["id"].string == group.id }?["label"].string ?? ""
        let commands = group.items.map { store.command($0["command"].string) }
        let labels = actionGroups(store.state["tool_actions"].array.filter { !$0["group"].isNull && !$0["segmented"].bool }).map { group in
            store.catalog["tool_action_groups"].array.first { $0["id"].string == group.id }?["label"].string ?? ""
        }
        return PropertyChoiceRow(labelWidth: store.panelLabelWidth(labels)) {
            Text(label).lineLimit(2)
            EditorChoice(label: label, options: commands.map { $0["label"].string }, selected: commands.firstIndex { $0["selected"].bool } ?? 0,
                identifier: "tool-action-group-" + group.id, background: palette.controlFill, height: store.panelMetrics.height, menu: { open in
                    open(AppleContextMenu(JSON(["sections": [commands.map { command in
                        ["label": command["label"].raw, "enabled": command["enabled"].raw, "selected": command["selected"].raw,
                         "identifier": "tool-action-" + command["id"].string, "action": ["type": "invoke", "command": command["id"].raw]]
                    }]])) { store.dispatch($0) })
                }) { store.invoke(commands[$0]["id"].string) }
        }
    }
    var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var settingGroups: [[JSON]] {
        settings.reduce(into: [[JSON]]()) { groups, item in
            if let last = groups.last?.last, last["group_id"].string == item["group_id"].string { groups[groups.count - 1].append(item) }
            else { groups.append([item]) }
        }
    }
    func setting(_ item: JSON, editingContext: String, valueOnly: Bool = false) -> some View {
        NumberControl(store: store, label: item["label"].string, value: item["value"].number,
            control: item["numeric"], identifier: "tool-" + item.settingID, valueOnly: valueOnly,
            gestureChange: item["preview"].bool ? { phase, value, completion in
                guard editingContext == context else { completion(nil); return }
                store.edit(["type": "tool_setting_contact", "id": item.settingID, "phase": phase, "value": value], completion: completion)
            } : nil, reset: {
                guard editingContext == context else { return }
                store.edit(["type": "reset_tool_setting", "id": item.settingID]) { _ in }
            }) { value, completion in
            guard editingContext == context else { completion(nil); return }
            store.edit(["type": "set_tool_setting", "id": item.settingID, "value": value], completion: completion)
        }.id(context + item.settingID + item["label"].string + item["numeric"].stableKey)
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
        let modes = store.state["tool_actions"].array.filter { $0["segmented"].bool }
        let bounds = settings.filter { ["tonal_lower", "tonal_upper"].contains($0["id"].string) }
        VStack(alignment: .leading, spacing: store.panelMetrics.spacing) {
            if !modes.isEmpty { SelectionModeGroup(store: store, actions: modes, height: store.panelMetrics.height, iconSize: store.panelMetrics.icon) }
            ForEach(store.state["tool_extra"].array.indices, id: \.self) { index in
                let choice = store.state["tool_extra"][index]["Choice"]
                if !choice.isNull {
                    SegmentedChoiceBar(choice: choice, prefix: "tool", height: store.panelMetrics.height, iconSize: store.panelMetrics.icon,
                        labeledCell: CGSize(width: store.panelMetrics.height, height: store.panelMetrics.height), palette: palette) { item in
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
                HStack(spacing: store.panelMetrics.spacing) {
                    Text(item["label"].string).lineLimit(2).frame(width: 62, alignment: .leading)
                    NumberControl(store: store, label: item["label"].string, value: item["value"].number,
                        control: item["numeric"], identifier: "tool-" + item.settingID, inline: true) { value, completion in
                        store.edit(["type": "set_tool_setting", "id": item.settingID, "value": value], completion: completion)
                    }
                }.frame(minHeight: store.panelMetrics.height).accessibilityElement(children: .contain).accessibilityIdentifier("tool-setting-" + item.settingID)
            }
        }
    }
}

private struct PickerSettingsRows: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        let picker=store.state["color_picker"],copy=store.catalog["native_copy"]["sampler"]
        let labels=[copy["source"].string,copy["sample_size"].string]
        let source=[copy["visible_color"].string]+(picker["can_sample_layer"].bool ? [copy["selected_layer"].string] : [])
        let sizes=picker["sample_sizes"].array.map(\.uint)
        VStack(alignment:.leading,spacing:store.panelMetrics.spacing) {
            PropertyChoiceRow(labelWidth:store.panelLabelWidth(labels)) {
                Text(labels[0]).lineLimit(2)
                EditorChoice(label:labels[0],options:source,selected:picker["layer"].bool ? 1 : 0,identifier:"picker-setting-source",background:EditorPalette(source:store.state["palette"]).controlFill,height:store.panelMetrics.height) {
                    store.dispatch(["type":"color_picker","action":["kind":"source","layer":$0==1]])
                }
            }
            PropertyChoiceRow(labelWidth:store.panelLabelWidth(labels)) {
                Text(labels[1]).lineLimit(2)
                EditorChoice(label:labels[1],options:sizes.map { width in copy["sizes"].array.first { $0[0].uint==width }?[1].string ?? "" },selected:sizes.firstIndex(of:picker["sample_width"].uint) ?? 0,identifier:"picker-setting-size",background:EditorPalette(source:store.state["palette"]).controlFill,height:store.panelMetrics.height) {
                    store.dispatch(["type":"set_color_sample_size","width":sizes[$0]])
                }
            }
        }
    }
}
