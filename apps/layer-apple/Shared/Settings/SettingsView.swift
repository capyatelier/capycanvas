import SwiftUI

struct SettingsView: View {
    @ObservedObject var store: EditorStore
    @FocusState private var searching: Bool
    @State private var numberResets: [String: UInt64] = [:]
    private var model: JSON { store.snapshot["preferences"] }
    private var page: JSON { model["pages"].array.first { $0["id"].string == model["page"].string } ?? JSON() }
    var body: some View {
        NavigationSplitView {
            VStack {
                EditorTextField("Search settings", value: model["query"].string) {
                    action(["type": "search", "query": $0])
                }
                    .textFieldStyle(.roundedBorder).padding(.horizontal).focused($searching)
                    .accessibilityIdentifier("settings-search")
                List(selection: Binding<String?>(get: { model["page"].string }, set: { if let next = $0 {
                    searching = false
                    action(["type": "page", "page": next])
                } })) {
                    ForEach(model["pages"].array.indices, id: \.self) { index in
                        let p = model["pages"][index]
                        HStack {
                            SharedIcon(name: p["icon"].string).accessibilityHidden(true)
                            Text(p["title"].string)
                        }.tag(p["id"].string)
                            .accessibilityIdentifier("settings-page-" + p["id"].string)
                    }
                }
            }.navigationTitle("Settings")
                .navigationSplitViewColumnWidth(min: 200, ideal: 220)
        } detail: {
            detail.navigationTitle(page["title"].string)
                .toolbar { ToolbarItem(placement: .confirmationAction) {
                    SettingsDoneButton(store: store)
                } }
        }.frame(minHeight: 420)
            #if os(macOS)
            .frame(minWidth: 560)
            #endif
    }
    @ViewBuilder private var detail: some View {
        if !model["query"].string.isEmpty {
            List {
                ForEach(model["search_results"].array.indices, id: \.self) { index in
                    let result = model["search_results"][index]
                    Button { searching = false; action(result["action"].object) } label: {
                        VStack(alignment: .leading) {
                            Text(result["title"].string)
                            Text(result["description"].string).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                if model["search_results"].array.isEmpty { Text("No matching settings") }
            }
        } else if model["page"].string == "shortcuts" {
            ShortcutSettingsView(store: store)
        } else {
            ScrollViewReader { scroll in
                Form {
                    ForEach(page["groups"].array.indices, id: \.self) { index in
                        let group = page["groups"][index]
                        Section(group["title"].string) {
                            ForEach(group["rows"].array.indices, id: \.self) { rowIndex in
                                let row = group["rows"][rowIndex]
                                if row["visible"].bool { preference(row).id(row["id"].string) }
                            }
                        }
                    }
                    if !model["error"].isNull { Text(model["error"].string).foregroundStyle(.red) }
                }.formStyle(.grouped)
                    .onChange(of: model["reveal"].string, initial: true) { _, id in
                        if !id.isEmpty { scroll.scrollTo(id) }
                    }
            }
        }
    }
    private func action(_ value: [String: Any]) {
        // Native search/selection bindings can finish while the sheet closes.
        guard !model.isNull else { return }
        store.dispatch(["type": "preferences", "action": value])
    }
    private func edit(_ row: JSON, _ value: Any) { action(["type": "edit", "id": row["id"].raw, "value": value]) }
    @ViewBuilder private func preference(_ row: JSON) -> some View {
        let kind = row["kind"]
        VStack(alignment: .leading, spacing: 4) {
            switch kind["type"].string {
            case "switch":
                Toggle(row["title"].string, isOn: Binding(get: { kind["active"].bool }, set: { edit(row, $0) }))
            case "choice":
                choice(row)
            case "text":
                PreferenceText(label: row["title"].string, value: kind["value"].string,
                    placeholder: kind["placeholder"].string, maxLength: Int(kind["max_length"].uint)) { edit(row, $0) }
            case "number":
                let reset = numberResets[row["id"].string, default: 0]
                NumberControl(store: store, label: row["title"].string, value: kind["value"].number, control: kind["control"]) { value, completion in
                    // Reset replaces this draft; a late focus callback from the
                    // discarded editor must not overwrite the shared default.
                    guard reset == numberResets[row["id"].string, default: 0] else { completion(nil); return }
                    store.edit(["type": "preferences", "action": ["type": "edit", "id": row["id"].raw, "value": value]], completion: completion)
                }.id(reset)
            case "info":
                LabeledContent(row["title"].string, value: kind["value"].string)
            case "link":
                if let url = URL(string: kind["url"].string) {
                    LabeledContent(row["title"].string) {
                        Link(kind["label"].string, destination: url)
                            .foregroundStyle(.tint)
                            .accessibilityIdentifier("preference-" + row["id"].string)
                    }
                }
            default: Text(row["title"].string)
            }
            if !row["description"].string.isEmpty { Text(row["description"].string).font(.caption).foregroundStyle(.secondary) }
        }.disabled(!row["enabled"].bool)
            .contextMenu {
                if !row["reset"].isNull {
                    Button(row["reset"]["label"].string) {
                        if kind["type"].string == "number" { numberResets[row["id"].string, default: 0] &+= 1 }
                        action(["type": "reset", "id": row["id"].raw])
                    }.disabled(!row["reset"]["enabled"].bool)
                }
            }
    }
    @ViewBuilder private func choice(_ row: JSON) -> some View {
        let kind = row["kind"]
        if kind["presentation"]["type"].string == "image_tiles" {
            Text(row["title"].string)
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(minimum: 0, maximum: 64), spacing: 6),
                count: max(1, Int(kind["presentation"]["columns"].uint))), spacing: 6) {
                ForEach(kind["options"].array.indices, id: \.self) { index in
                    let selected = index == Int(kind["selected"].number)
                    IconTile(icon: kind["icons"][index].string, label: kind["options"][index].string,
                        selected: selected, size: 48, background: Color.primary.opacity(0.05)) { edit(row, index) }
                        .frame(height: 64)
                        .overlay { RoundedRectangle(cornerRadius: 6)
                            .strokeBorder(selected ? EditorPalette.sharedAccent : .clear, lineWidth: 2).allowsHitTesting(false) }
                        .accessibilityIdentifier("preference-" + row["id"].string + "-\(index)")
                }
            }.padding(.vertical, 6)
        } else {
            Picker(row["title"].string, selection: Binding(get: { Int(kind["selected"].number) }, set: { edit(row, $0) })) {
                ForEach(kind["options"].array.indices, id: \.self) { index in
                    Label { Text(kind["options"][index].string) } icon: {
                        if !kind["icons"][index].string.isEmpty { SharedIcon(name: kind["icons"][index].string) }
                    }.tag(index)
                }
            }.accessibilityIdentifier("preference-" + row["id"].string)
        }
    }
}

private struct SettingsDoneButton: View {
    let store: EditorStore
    // Observing a child's focused callback must not invalidate the form that
    // publishes it; a native text field can otherwise trigger a focus loop.
    @FocusedValue(\.editorTextCommit) private var commitText
    var body: some View {
        Button("Done") {
            commitText?()
            store.dispatch(["type": "close_settings"])
        }.accessibilityIdentifier("settings-done")
    }
}

private struct PreferenceText: View {
    let label: String
    let value: String
    let placeholder: String
    let maxLength: Int
    let commit: (String) -> Void
    @State private var text = ""
    @FocusState private var editing: Bool
    var body: some View {
        // Keep the label outside the native editor's hit area so its Reset menu
        // remains available while the field owns text selection and editing.
        HStack {
            Text(label)
            Spacer()
            field.labelsHidden().accessibilityLabel(label)
                .multilineTextAlignment(.trailing).frame(width: 132)
        }
    }
    private var field: some View {
        TextField(label, text: $text, prompt: Text(placeholder)).focused($editing).onSubmit { commit(text) }
            .autocorrectionDisabled()
            #if os(iOS)
            .textInputAutocapitalization(.never).keyboardType(.asciiCapable)
            #endif
            .focusedValue(\.editorTextCommit, { commit(text) })
            .onAppear { text = value }.onChange(of: value) { _, next in text = next }
            .onChange(of: text) { _, next in
                if next.count > maxLength { text = String(next.prefix(maxLength)) }
            }
            .onChange(of: editing) { old, next in if old && !next { commit(text) } }
    }
}
