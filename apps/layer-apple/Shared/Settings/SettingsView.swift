import SwiftUI

struct SettingsView: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["preferences"] }
    private var page: JSON { model["pages"].array.first { $0["id"].string == model["page"].string } ?? JSON() }
    var body: some View {
        NavigationSplitView {
            VStack {
                TextField("Search settings", text: Binding(get: { model["query"].string },
                    set: { action(["type": "search", "query": $0]) }))
                    .textFieldStyle(.roundedBorder).padding(.horizontal).accessibilityIdentifier("settings-search")
                List(selection: Binding<String?>(get: { model["page"].string }, set: { if let next = $0 { action(["type": "page", "page": next]) } })) {
                    ForEach(model["pages"].array.indices, id: \.self) { index in
                        let p = model["pages"][index]
                        HStack { SharedIcon(name: p["icon"].string); Text(p["title"].string) }.tag(p["id"].string)
                    }
                }
            }.navigationTitle("Settings")
        } detail: {
            detail.navigationTitle(page["title"].string)
                .toolbar { ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { store.dispatch(["type": "close_settings"]) }.accessibilityIdentifier("settings-done")
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
                    Button { action(result["action"].object) } label: {
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
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    private func edit(_ row: JSON, _ value: Any) { action(["type": "edit", "id": row["id"].raw, "value": value]) }
    @ViewBuilder private func preference(_ row: JSON) -> some View {
        let kind = row["kind"]
        VStack(alignment: .leading, spacing: 4) {
            switch kind["type"].string {
            case "switch":
                Toggle(row["title"].string, isOn: Binding(get: { kind["active"].bool }, set: { edit(row, $0) }))
            case "choice":
                Picker(row["title"].string, selection: Binding(get: { Int(kind["selected"].number) }, set: { edit(row, $0) })) {
                    ForEach(kind["options"].array.indices, id: \.self) { index in Text(kind["options"][index].string).tag(index) }
                }.accessibilityIdentifier("preference-" + row["id"].string)
            case "text":
                PreferenceText(label: row["title"].string, value: kind["value"].string) { edit(row, $0) }
            case "number":
                NumberControl(store: store, label: row["title"].string, value: kind["value"].number, control: kind["control"]) { value, completion in
                    store.edit(["type": "preferences", "action": ["type": "edit", "id": row["id"].raw, "value": value]], completion: completion)
                }
            case "info":
                LabeledContent(row["title"].string, value: kind["value"].string)
            case "link":
                if let url = URL(string: kind["url"].string) { Link(kind["label"].string, destination: url) }
            default: Text(row["title"].string)
            }
            if !row["description"].string.isEmpty { Text(row["description"].string).font(.caption).foregroundStyle(.secondary) }
        }.disabled(!row["enabled"].bool)
            .contextMenu {
                if !row["reset"].isNull { Button(row["reset"]["label"].string) { action(["type": "reset", "id": row["id"].raw]) }.disabled(!row["reset"]["enabled"].bool) }
            }
    }
}

private struct PreferenceText: View {
    let label: String
    let value: String
    let commit: (String) -> Void
    @State private var text = ""
    @FocusState private var editing: Bool
    var body: some View {
        TextField(label, text: $text).focused($editing).onSubmit { commit(text) }
            .onAppear { text = value }.onChange(of: value) { _, next in if !editing { text = next } }
            .onChange(of: editing) { old, next in if old && !next { commit(text) } }
    }
}
