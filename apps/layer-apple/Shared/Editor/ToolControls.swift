import SwiftUI

/// Tool families, subtools and settings are live shared models, also used by
/// the desktop host. This includes non-paint tools and their command actions.
struct ToolSetControls: View {
    @ObservedObject var store: EditorStore
    var panel = "brushes"
    private var model: JSON { panel == "brushes" ? store.state["tool_set"] : store.state["tool_panels"][panel] }
    private var sets: Bool { panel == "brush_sets" || panel == "sculpt_sets" }
    var body: some View {
        VStack(spacing: 8) {
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
                        HStack(spacing: 6) {
                            SharedIcon(name: item["icon"].string)
                            Text(item["label"].string).fontWeight(.bold).lineLimit(1)
                            Spacer(minLength: 0)
                        }.padding(.horizontal, 6).frame(minHeight: 44)
                    } else if group {
                        VStack(spacing: 8) {
                            SharedIcon(name: item["icon"].string)
                            Text(item["label"].string).fontWeight(.bold)
                                .frame(minHeight: store.catalog["text_size_pt"].number * 4 / 3 * 0.85 * 1.66)
                        }.font(.system(size: store.catalog["text_size_pt"].number * 4 / 3 * 0.85))
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
                                    .frame(maxWidth: .infinity, minHeight: store.catalog["text_size_pt"].number * 4 / 3 * 1.66,
                                        alignment: item["preview"].isNull ? .leading : .trailing)
                            }
                        }
                    }
                }.padding(.horizontal, group ? 0 : 6).padding(.vertical, group ? 4 : 3)
                    .frame(maxWidth: .infinity, minHeight: group ? nil : item["preview"].isNull ? 44
                        : store.catalog["text_size_pt"].number * 4 / 3 * 1.66 + 8)
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

/// Equal flexible buttons with wrapping, matching the web tool-group rows.
/// An adaptive grid reserves unused columns when a family has fewer groups.
private struct ToolGroupsLayout: Layout {
    private func rows(_ width: CGFloat, _ subviews: Subviews) -> [(Range<Int>, CGFloat, CGFloat)] {
        let columns = max(1, Int((width + 4) / 74))
        return stride(from: 0, to: subviews.count, by: columns).map { start in
            let range = start..<min(start + columns, subviews.count)
            let cell = max(0, (width - CGFloat(range.count - 1) * 4) / CGFloat(range.count))
            let height = range.map { subviews[$0].sizeThatFits(ProposedViewSize(width: cell, height: nil)).height }.max() ?? 0
            return (range, cell, height)
        }
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let requested = proposal.width ?? 226
        let width = requested.isFinite ? max(0, requested) : 226
        let rows = rows(width, subviews)
        return CGSize(width: width, height: rows.reduce(0) { $0 + $1.2 } + CGFloat(max(0, rows.count - 1)) * 4)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for (range, width, height) in rows(bounds.width, subviews) {
            for index in range {
                subviews[index].place(at: CGPoint(x: bounds.minX + CGFloat(index - range.lowerBound) * (width + 4), y: y),
                    anchor: .topLeading, proposal: ProposedViewSize(width: width, height: height))
            }
            y += height + 4
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
            + ":" + selected + ":" + String(store.state["brush"]["preset"].uint)
            + ":" + String(store.state["layer_tools"]["editing_layer"]["id"].uint)
            + ":" + String(store.state["layer_tools"]["editing_layer"]["mask_selected"].bool)
    }
    var body: some View {
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
