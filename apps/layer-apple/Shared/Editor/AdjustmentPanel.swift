import SwiftUI

struct AdjustmentPanel: View {
    @ObservedObject var store: EditorStore
    var splitPicker = false
    @State private var projection = UUID().uuidString
    @Environment(\.displayScale) private var scale
    @Environment(\.scenePhase) private var phase
    @FocusState private var searching: Bool
    private var picker: JSON { store.state["filter_picker"] }
    private var choices: [JSON] { store.state["adjustments"].array }
    private var categories: [JSON] { store.state["filter_categories"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func send(_ action: [String: Any]) { store.dispatch(["type": "filter_picker", "action": action]) }
    var body: some View {
        VStack(spacing: 0) {
            if !splitPicker { HStack(spacing: 6) {
                if !picker["search"].isNull {
                    EditorTextField(picker["search_label"].string, value: picker["search"].string) {
                        send(["op": "search", "query": $0])
                    }
                        .textFieldStyle(.plain).padding(.horizontal, 12).frame(height: 34)
                        .background(palette["input"], in: RoundedRectangle(cornerRadius: 2))
                        .overlay(RoundedRectangle(cornerRadius: 2).stroke(palette["text"].opacity(0.4), lineWidth: 1))
                        .focused($searching).accessibilityIdentifier("filter-search")
                        .onKeyPress(.escape) { send(["op": "toggle_search"]); return .handled }
                } else {
                    SharedIcon(name: categories.first { $0["id"].string == picker["category"].string }?["icon"].string ?? "adjustments")
                    EditorChoice(label: "Category", options: categories.map { $0["label"].string },
                        selected: categories.firstIndex { $0["id"].stableKey == picker["category"].stableKey } ?? 0,
                        identifier: "filter-category", background: palette["input"], bold: true) {
                        send(["op": "category", "category": categories[$0]["id"].raw])
                    }.frame(maxWidth: .infinity)
                }
                IconTile(icon: "search", label: picker["search_label"].string) {
                    send(["op": "toggle_search"])
                }.frame(width: 48, height: 34).accessibilityIdentifier("filter-search-toggle")
            }.frame(minHeight: 34).modifier(PanelBodyMeasurement(panel: "adjustments", part: "header")) }
            GeometryReader { viewport in
                EditorScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(choices.indices, id: \.self) { index in
                            let choice = choices[index]
                            if !splitPicker && (index == 0 || choice["category"].string != choices[index - 1]["category"].string) {
                                HStack(spacing: 6) {
                                    SharedIcon(name: choice["category_icon"].string)
                                    Text(choice["category_label"].string).fontWeight(.bold)
                                }.frame(height: 17).foregroundStyle(palette["text"].opacity(0.55)).padding(8)
                            }
                            AdjustmentRow(store: store, previews: store.filterPreviews, choice: choice)
                                .modifier(PanelBodyMeasurement(panel: "adjustments", part: "row-unit", kind: .unit))
                                .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(projection)) } action: { rect in
                                    let token = projection + ":" + choice["id"].string
                                    if rect.intersects(CGRect(origin: .zero, size: viewport.size)) {
                                        store.filterPreviews.show(token: token, id: choice["id"].string, width: max(0, rect.width - 12), scale: scale)
                                    } else { store.filterPreviews.hide(token) }
                                }
                                .onDisappear { store.filterPreviews.hide(projection + ":" + choice["id"].string) }
                                .id(choice["id"].string)
                        }
                        if choices.isEmpty {
                            Text(picker["empty_label"].string)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.vertical, store.catalog["text_size_pt"].number * 4 / 3)
                        }
                    }.modifier(PanelBodyMeasurement(panel: "adjustments", part: "choices", kind: .scroll))
                }.coordinateSpace(name: projection).accessibilityIdentifier("filter-list")
            }
        }.padding(6)
            .modifier(PanelBodyMeasurement(panel: "adjustments", part: "insets", intrinsicHeight: 12))
            .onChange(of: picker["search"].isNull) { _, closed in searching = !closed }
            .onAppear { store.filterPreviews.setActive(phase != .background) }
            .onChange(of: phase) { _, phase in store.filterPreviews.setActive(phase != .background) }
            .onDisappear { store.filterPreviews.hidePanel(projection) }
    }
}

private struct AdjustmentRow: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var previews: FilterPreviews
    let choice: JSON
    var body: some View {
        Button { store.dispatch(choice["action"]) } label: {
            VStack(spacing: 0) {
                if let image = previews.images[choice["id"].string] {
                    Image(decorative: image, scale: 1).resizable().frame(height: 40)
                        .accessibilityIdentifier("filter-preview-" + choice["id"].string)
                } else { Color.clear.frame(height: 40) }
                HStack(spacing: 6) {
                    Spacer(minLength: 0)
                    if choice["animated"].bool { SharedIcon(name: "animation", size: 12).opacity(0.55) }
                    SharedIcon(name: choice["icon"].string)
                    Text(choice["label"].string).lineLimit(1)
                }.frame(height: 18)
            }.padding(.horizontal, 6).padding(.vertical, 3).contentShape(RoundedRectangle(cornerRadius: 6))
        }.buttonStyle(EditorControlButtonStyle(selected: store.state["filter_picker"]["selected"].string == choice["id"].string))
            .help(choice["tooltip"].string)
            .accessibilityAddTraits(store.state["filter_picker"]["selected"].string == choice["id"].string ? .isSelected : [])
            .accessibilityIdentifier("adjustment-" + choice["id"].string)
            .accessibilityValue(previews.images[choice["id"].string] == nil ? "Preview pending" : "Preview ready")
    }
}

struct FilterTypesPanel: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        VStack(spacing: 6) {
            EditorScrollView {
                VStack(spacing: 2) {
                    ForEach(store.state["filter_categories"].array.indices, id: \.self) { index in
                        let category = store.state["filter_categories"][index]
                        Button { store.dispatch(["type": "filter_picker", "action": ["op": "category", "category": category["id"].raw]]) } label: {
                            HStack(spacing: 6) {
                                SharedIcon(name: category["icon"].string)
                                Text(category["label"].string).lineLimit(1)
                                Spacer(minLength: 0)
                            }.padding(.horizontal, 6).frame(minHeight: 44).contentShape(Rectangle())
                        }.buttonStyle(EditorControlButtonStyle(selected: category["id"].stableKey == store.state["filter_picker"]["category"].stableKey))
                            .accessibilityIdentifier("filter-type-" + category["id"].string)
                    }
                }.modifier(PanelBodyMeasurement(panel: "filter_types", part: "choices", kind: .scroll))
            }
            Button("Cancel") { store.dispatch(["type": "effect", "action": ["op": "cancel_filter"]]) }
                .accessibilityIdentifier("cancel-filter")
                .modifier(PanelBodyMeasurement(panel: "filter_types", part: "footer"))
        }.padding(8).modifier(PanelBodyMeasurement(panel: "filter_types", part: "insets", intrinsicHeight: 22))
    }
}
