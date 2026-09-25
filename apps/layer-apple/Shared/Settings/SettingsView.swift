import SwiftUI

struct SettingsView: View {
    @ObservedObject var store: EditorStore
    @Environment(\.openURL) private var openURL
    @FocusState private var searching: Bool
    @State private var numberResets: [String: UInt64] = [:]
    @State private var linkFailed = false
    @State private var profiles = false
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
                            .background(NativePenScroll().frame(width: 0, height: 0))
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
        }.sheet(isPresented: $profiles) { ColorProfileLibrary(preferences: store.colorPreferences).modifier(EditorPopupPresentation()) }
            .frame(minHeight: 420)
            #if os(macOS)
            .frame(minWidth: 560)
            #endif
            .environment(\.openURL, OpenURLAction { url in
                openURL(url) { accepted in linkFailed = !accepted }
                return .handled
            })
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
                    }.background(NativePenScroll().frame(width: 0, height: 0))
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
                                if row["visible"].bool {
                                    preference(row).id(row["id"].string)
                                        .background(NativePenScroll().frame(width: 0, height: 0))
                                }
                            }
                        }
                    }
                    if model["page"].string == "color" {
                        Section("Display Details") {
                            LabeledContent("Canvas and color previews", value: store.snapshot["color_panel"]["hdr"].bool ? "Extended linear sRGB · HDR" : "Display P3 · SDR")
                            if store.snapshot["color_panel"]["hdr"].bool {
                                LabeledContent("Current display headroom", value: String(format: "%.1f× SDR white", store.displayHeadroom))
                            }
                            LabeledContent("Screen", value: store.displayDetails.screen)
                            LabeledContent("Display conversion", value: store.displayDetails.destination)
                            Text("The system converts tagged colors for this display, including sRGB screens. Document and export colors are independent of the screen.").font(.caption).foregroundStyle(.secondary)
                        }
                        Button("Manage Color Profiles…") { profiles = true }
                            .accessibilityIdentifier("settings-color-profiles")
                    }
                    if !model["error"].isNull { Text(model["error"].string).foregroundStyle(.red) }
                    if linkFailed { Text("Could not open the link").foregroundStyle(.red) }
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
            case "number":
                let reset = numberResets[row["id"].string, default: 0]
                NumberControl(store: store, label: row["title"].string, value: kind["value"].number, control: kind["control"]) { value, completion in
                    // Reset replaces this draft; a late focus callback from the
                    // discarded editor must not overwrite the shared default.
                    guard reset == numberResets[row["id"].string, default: 0] else { completion(nil); return }
                    store.edit(["type": "preferences", "action": ["type": "edit", "id": row["id"].raw, "value": value]], completion: completion)
                }.id(reset)
            case "swatches":
                PreferenceSwatches(row: row, palette: EditorPalette(source: store.state["palette"])) { edit(row, $0) }
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
                        selected: selected, size: 48, background: Color.primary.opacity(0.05), corner: .radius(6)) { edit(row, index) }
                        .frame(height: 64)
                        .overlay { SquircleShape(6)
                            .strokeBorder(selected ? EditorPalette(source: store.state["palette"]).accent : .clear, lineWidth: 2).allowsHitTesting(false) }
                        .accessibilityIdentifier("preference-" + row["id"].string + "-\(index)")
                }
            }.padding(.vertical, 6)
        } else {
            Picker(row["title"].string, selection: Binding(get: { Int(kind["selected"].number) }, set: { edit(row, $0) })) {
                ForEach(kind["options"].array.indices, id: \.self) { index in
                    if kind["icons"][index].string.isEmpty {
                        Text(kind["options"][index].string).tag(index)
                    } else {
                        Label(kind["options"][index].string,
                            image: "icon-" + SharedIcon.assetKey(kind["icons"][index].string)).tag(index)
                    }
                }
            }.accessibilityIdentifier("preference-" + row["id"].string)
                .accessibilityValue(kind["options"][Int(kind["selected"].number)].string)
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

private struct PreferenceSwatches: View {
    let row: JSON
    let palette: EditorPalette
    let commit: (String) -> Void
    @State private var editing = false
    @State private var text = ""
    @FocusState private var focused: Bool
    private var kind: JSON { row["kind"] }
    var body: some View {
        if kind["inline"].bool {
            HStack(spacing: 8) {
                Text(row["title"].string).layoutPriority(1)
                Spacer(minLength: 8)
                if editing { field.frame(width: 96) }
                HStack(spacing: 10) { circles(28) }
            }
        } else {
            VStack(spacing: 12) {
                Text(row["title"].string).frame(maxWidth: .infinity, alignment: .leading)
                CenteredFlow(spacing: 10) { circles(32) }
                if editing { field.frame(width: 120) }
            }
        }
    }
    private func circles(_ side: CGFloat) -> some View {
        ForEach(kind["swatches"].array.indices, id: \.self) { index in
            let swatch = kind["swatches"][index], selected = Int(kind["selected"].uint) == index
            Button {
                if swatch["custom"].bool {
                    text = kind["custom"].string; editing = true; focused = true
                } else {
                    editing = false; commit(swatch["value"].string)
                }
            } label: {
                ZStack {
                    if swatch["color"].isNull {
                        Circle().fill(palette["text"].opacity(0.10))
                        Circle().strokeBorder(palette["text"].opacity(0.15), lineWidth: 1)
                    } else {
                        Circle().fill(Color(hex: swatch["color"].string))
                    }
                    if !swatch["icon"].isNull && !(selected && !swatch["custom"].bool) {
                        SharedIcon(name: swatch["icon"].string, size: 16)
                            .foregroundStyle(swatch["foreground"].isNull ? palette["text"] : Color(hex: swatch["foreground"].string))
                    } else if selected && !swatch["custom"].bool {
                        SharedIcon(name: "check", size: 16)
                            .foregroundStyle(swatch["foreground"].isNull ? palette["text"] : Color(hex: swatch["foreground"].string))
                    }
                }.frame(width: side, height: side)
                    .overlay { if selected { Circle().strokeBorder(palette["text"], lineWidth: 2).padding(-4) } }
                    .contentShape(Circle())
            }.buttonStyle(.plain)
                .accessibilityLabel(swatch["label"].string)
                .accessibilityAddTraits(selected ? .isSelected : [])
                .accessibilityIdentifier("preference-" + row["id"].string + "-\(index)")
        }
    }
    private var field: some View {
        TextField(row["title"].string, text: $text, prompt: Text(kind["placeholder"].string))
            .labelsHidden().focused($focused).monospaced()
            .onSubmit { submit() }
            .autocorrectionDisabled()
            #if os(iOS)
            .textInputAutocapitalization(.never).keyboardType(.asciiCapable)
            #endif
            .focusedValue(\.editorTextCommit, { submit() })
            .onChange(of: text) { _, next in if next.count > 7 { text = String(next.prefix(7)) } }
            .onChange(of: focused) { old, next in if old && !next { submit() } }
            .onChange(of: kind["value"].string) { _, _ in editing = false }
            .accessibilityIdentifier("preference-" + row["id"].string + "-custom")
    }
    private func submit() {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard trimmed != kind["custom"].string else { return }
        commit(trimmed)
        text = kind["custom"].string
    }
}

private struct CenteredFlow: Layout {
    let spacing: CGFloat
    private func rows(_ width: CGFloat, _ subviews: Subviews) -> [[(Int, CGSize)]] {
        var rows: [[(Int, CGSize)]] = [[]], used: CGFloat = 0
        for (index, view) in subviews.enumerated() {
            let size = view.sizeThatFits(.unspecified)
            if !rows[rows.count - 1].isEmpty && used + spacing + size.width > width { rows.append([]); used = 0 }
            used += (rows[rows.count - 1].isEmpty ? 0 : spacing) + size.width
            rows[rows.count - 1].append((index, size))
        }
        return rows
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let lines = rows(width, subviews)
        let height = lines.map { $0.map(\.1.height).max() ?? 0 }.reduce(0, +) + spacing * CGFloat(max(0, lines.count - 1))
        let widest = lines.map { $0.map(\.1.width).reduce(0, +) + spacing * CGFloat(max(0, $0.count - 1)) }.max() ?? 0
        return CGSize(width: proposal.width ?? widest, height: height)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for line in rows(bounds.width, subviews) {
            let width = line.map(\.1.width).reduce(0, +) + spacing * CGFloat(max(0, line.count - 1))
            var x = bounds.midX - width / 2
            let height = line.map(\.1.height).max() ?? 0
            for (index, size) in line {
                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += height + spacing
        }
    }
}

