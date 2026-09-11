import SwiftUI

struct AdjustmentPanel: View {
    @ObservedObject var store: EditorStore
    @State private var projection = UUID().uuidString
    @Environment(\.displayScale) private var scale
    @FocusState private var searching: Bool
    private var picker: JSON { store.state["filter_picker"] }
    private var choices: [JSON] { store.state["adjustments"].array }
    private var categories: [JSON] { store.state["filter_categories"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func send(_ action: [String: Any]) { store.dispatch(["type": "filter_picker", "action": action]) }
    var body: some View {
        VStack(spacing: 6) {
            HStack(spacing: 6) {
                if !picker["search"].isNull {
                    TextField(picker["search_label"].string, text: Binding(get: { picker["search"].string },
                        set: { send(["op": "search", "query": $0]) }))
                        .textFieldStyle(.plain).padding(6).background(palette["input"], in: RoundedRectangle(cornerRadius: 6))
                        .focused($searching).accessibilityIdentifier("filter-search")
                } else {
                    Menu {
                        ForEach(categories.indices, id: \.self) { index in
                            Button(categories[index]["label"].string) {
                                send(["op": "category", "category": categories[index]["id"].raw])
                            }
                        }
                    } label: {
                        HStack {
                            Text(categories.first { $0["id"].string == picker["category"].string }?["label"].string ?? "")
                                .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                            SharedIcon(name: "chevron-down")
                        }.padding(6).background(palette["input"], in: RoundedRectangle(cornerRadius: 6))
                    }.menuStyle(.borderlessButton).menuIndicator(.hidden).accessibilityIdentifier("filter-category")
                }
                IconTile(icon: "search", label: picker["search_label"].string, selected: !picker["search"].isNull) {
                    send(["op": "toggle_search"])
                }.frame(width: 34, height: 34).accessibilityIdentifier("filter-search-toggle")
            }.frame(minHeight: 34)
            GeometryReader { viewport in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(choices.indices, id: \.self) { index in
                            let choice = choices[index]
                            if index == 0 || choice["category"].string != choices[index - 1]["category"].string {
                                Text(choice["category_label"].string).fontWeight(.bold).foregroundStyle(palette["text"].opacity(0.55))
                                    .padding(8)
                            }
                            AdjustmentRow(store: store, previews: store.filterPreviews, choice: choice)
                                .onGeometryChange(for: CGRect.self) { $0.frame(in: .named(projection)) } action: { rect in
                                    let token = projection + ":" + choice["id"].string
                                    if rect.intersects(CGRect(origin: .zero, size: viewport.size)) {
                                        store.filterPreviews.show(token: token, id: choice["id"].string, width: max(0, rect.width - 12), scale: scale)
                                    } else { store.filterPreviews.hide(token) }
                                }
                                .onDisappear { store.filterPreviews.hide(projection + ":" + choice["id"].string) }
                                .id(choice["id"].string)
                        }
                        if choices.isEmpty { Text(picker["empty_label"].string).foregroundStyle(palette["text"].opacity(0.55)).padding(8) }
                    }
                }.coordinateSpace(name: projection).accessibilityIdentifier("filter-list")
            }
        }.padding(6)
            .onChange(of: picker["search"].isNull) { _, closed in searching = !closed }
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
                HStack(spacing: 4) {
                    Spacer(minLength: 0)
                    if choice["animated"].bool { SharedIcon(name: "animation", size: 12).opacity(0.55) }
                    Text(choice["label"].string).lineLimit(1)
                }
            }.padding(.horizontal, 6).padding(.vertical, 3).contentShape(RoundedRectangle(cornerRadius: 6))
        }.buttonStyle(.plain).help(choice["tooltip"].string)
            .accessibilityIdentifier("adjustment-" + choice["id"].string)
            .accessibilityValue(previews.images[choice["id"].string] == nil ? "Preview pending" : "Preview ready")
    }
}
