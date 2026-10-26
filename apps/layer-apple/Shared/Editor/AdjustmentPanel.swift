import SwiftUI

struct AdjustmentPanel: View {
    @ObservedObject var store: EditorStore
    var splitPicker = false
    @State private var projection = UUID().uuidString
    @Environment(\.scenePhase) private var phase
    @FocusState private var searching: Bool
    private var picker: JSON { store.state["filter_picker"] }
    private var choices: [JSON] { store.state["adjustments"].array }
    private var categories: [JSON] { store.state["filter_categories"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func send(_ action: [String: Any]) { store.dispatch(["type": "filter_picker", "action": action]) }
    var body: some View {
        VStack(spacing: 0) {
            if !splitPicker { HStack(spacing: store.panelMetrics.spacing) {
                if !picker["search"].isNull {
                    EditorTextField(picker["search_label"].string, value: picker["search"].string) {
                        send(["op": "search", "query": $0])
                    }
                        .editorSearchInput().textFieldStyle(.plain).padding(.horizontal, store.panelMetrics.padding).frame(height: store.panelMetrics.height)
                        .background(palette["input"], in: SquircleShape.control)
                        .overlay(SquircleShape.control.stroke(palette["text"].opacity(0.4), lineWidth: 1))
                        .focused($searching).accessibilityIdentifier("filter-search")
                        .onKeyPress(.escape) { send(["op": "toggle_search"]); return .handled }
                } else {
                    SharedIcon(name: categories.first { $0["id"].string == picker["category"].string }?["icon"].string ?? "adjustments")
                    EditorChoice(label: store.catalog["native_copy"]["color"]["category"].string, options: categories.map { $0["label"].string },
                        selected: categories.firstIndex { $0["id"].stableKey == picker["category"].stableKey } ?? 0,
                        identifier: "filter-category", background: palette.controlFill, bold: true, height: store.panelMetrics.height) {
                        send(["op": "category", "category": categories[$0]["id"].raw])
                    }.frame(maxWidth: .infinity)
                }
                IconTile(icon: "search", label: picker["search_label"].string) {
                    send(["op": "toggle_search"])
                }.frame(width: store.panelMetrics.height, height: store.panelMetrics.height).accessibilityIdentifier("filter-search-toggle")
            }.frame(minHeight: store.panelMetrics.height).modifier(PanelBodyMeasurement(panel: "adjustments", part: "header")) }
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
                                    if !choice["bundled_preview"].bool && rect.intersects(CGRect(origin: .zero, size: viewport.size)) {
                                        store.filterPreviews.show(token: token, id: choice["id"].string)
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
        }.padding(store.panelMetrics.inset)
            .modifier(PanelBodyMeasurement(panel: "adjustments", part: "insets", intrinsicHeight: store.panelMetrics.inset * 2))
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
            VStack(spacing: 4) {
                Group {
                    if choice["bundled_preview"].bool {
                        BundledPreview(name: "filter-" + choice["id"].string)
                    } else if let image = previews.images[choice["id"].string] {
                        Image(decorative: image, scale: 1).resizable()
                    } else { Color.clear }
                }.aspectRatio(6, contentMode: .fit).frame(maxWidth: .infinity)
                    .accessibilityIdentifier("filter-preview-" + choice["id"].string)
                HStack(spacing: 6) {
                    SharedIcon(name: choice["icon"].string)
                    if choice["animated"].bool { SharedIcon(name: "animation", size: 12).opacity(0.55) }
                    Text(choice["label"].string).fontWeight(.bold).lineLimit(1).truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }.frame(height: 18)
            }.padding(.horizontal, 6).padding(.vertical, 3).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle(selected: store.state["filter_picker"]["selected"].string == choice["id"].string))
            .help(choice["tooltip"].string)
            .accessibilityAddTraits(store.state["filter_picker"]["selected"].string == choice["id"].string ? .isSelected : [])
            .accessibilityIdentifier("adjustment-" + choice["id"].string)
            .accessibilityValue(store.catalog["native_copy"]["color"][choice["bundled_preview"].bool || previews.images[choice["id"].string] != nil ? "preview_ready" : "preview_pending"].string)
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
            Button(store.bootstrap["common"]["cancel"].string) { store.dispatch(["type": "effect", "action": ["op": "cancel_filter"]]) }
                .accessibilityIdentifier("cancel-filter")
                .modifier(PanelBodyMeasurement(panel: "filter_types", part: "footer"))
        }.padding(8).modifier(PanelBodyMeasurement(panel: "filter_types", part: "insets", intrinsicHeight: 22))
    }
}
