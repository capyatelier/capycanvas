import SwiftUI
import UniformTypeIdentifiers

/// Rust supplies searchable actions, bindings, limits and conflict resolution.
struct ShortcutSettingsView: View {
    @ObservedObject var store: EditorStore
    @State private var handledRequests: Set<UInt64> = []
    @State private var exporting: KeymapDocument?
    @State private var exportName = ""
    @State private var importing = false
    private var model: JSON { store.snapshot["preferences"] }
    private var page: JSON { model["shortcut_page"] }
    private var keymap: JSON { model["keymap"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    private var keymapRequests: [JSON] {
        store.state["requests"].array.filter { ["export_keymap", "import_keymap"].contains($0["kind"]["type"].string) }
    }
    var body: some View {
        Group {
            if !model["modifier_editor"].isNull {
                VStack(spacing: 0) { ModifierKeyPane(store: store) }
            } else if !page["category"].isNull {
                VStack(spacing: 0) { CategoryPane(store: store) }
            } else {
                RootPane(store: store)
            }
        }
        .sheet(isPresented: Binding(get: { !model["shortcut_editor"].isNull }, set: { if !$0 { closeEditor() } })) {
            ShortcutEditorSheet(store: store).modifier(EditorPopupPresentation())
        }
        .sheet(isPresented: Binding(get: { model["capture"]["id"].string == "modifier" }, set: { if !$0 { action(["type": "cancel_shortcut"]) } })) {
            ModifierCaptureSheet(store: store).modifier(EditorPopupPresentation())
        }
        .background {
            Color.clear
                .sheet(isPresented: Binding(get: { keymap["details"].bool }, set: { if !$0 { action(["type": "keymap_details", "open": false]) } })) {
                    KeymapDetailsSheet(store: store).modifier(EditorPopupPresentation())
                }
            Color.clear
                .sheet(isPresented: Binding(get: { !keymap["import"].isNull }, set: { if !$0 { action(["type": "cancel_keymap_import"]) } })) {
                    KeymapImportSheet(store: store).modifier(EditorPopupPresentation())
                }
        }
        .fileExporter(isPresented: Binding(get: { exporting != nil }, set: { if !$0 { exporting = nil } }),
            document: exporting, contentType: .capyKeymap, defaultFilename: exportName) { _ in exporting = nil }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.capyKeymap, .json]) { result in
            guard case .success(let url) = result else { return }
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let data = try? Data(contentsOf: url), data.count <= 1 << 20, let text = String(data: data, encoding: .utf8) else {
                store.failure = "The keymap file could not be read"; return
            }
            action(["type": "import_keymap", "text": text])
        }
        .onChange(of: keymapRequests.map { $0["id"].uint }, initial: true) { _, _ in
            for request in keymapRequests where !handledRequests.contains(request["id"].uint) {
                let id = request["id"].uint
                handledRequests.insert(id)
                store.dispatch(["type": "complete_request", "id": id, "error": NSNull()])
                if request["kind"]["type"].string == "export_keymap" {
                    exportName = (request["kind"]["name"].string as NSString).deletingPathExtension
                    exporting = KeymapDocument(text: request["kind"]["text"].string)
                } else {
                    importing = true
                }
            }
        }
    }
    private func closeEditor() {
        action(["type": model["capture"].isNull ? "close_shortcut_editor" : "cancel_shortcut"])
    }
}

private struct RootPane: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["preferences"] }
    private var page: JSON { model["shortcut_page"] }
    private var keymap: JSON { model["keymap"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        Form {
            Section("Keymap") {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Preset")
                        if keymap["outdated"].bool { Text("Updated since you chose it").font(.caption).foregroundStyle(.secondary) }
                    }
                    Spacer()
                    Picker("Keymap preset", selection: Binding(get: { keymap["selected"].string },
                        set: { action(["type": "select_keymap", "id": $0]) })) {
                        ForEach(keymap["presets"].array, id: \.shortcutID) { preset in
                            Text(preset["title"].string).tag(preset["id"].string)
                        }
                    }.labelsHidden().fixedSize().accessibilityIdentifier("keymap-preset")
                    Menu {
                        Button("Import…") { action(["type": "choose_keymap_file"]) }.accessibilityIdentifier("keymap-import-button")
                        Button("Export…") { action(["type": "export_keymap"]) }.accessibilityIdentifier("keymap-export-button")
                        Button("Differences…") { action(["type": "keymap_details", "open": true]) }.accessibilityIdentifier("keymap-details-button")
                        Divider()
                        Button("Reset All Shortcuts", role: .destructive) { action(["type": "reset_all_shortcuts"]) }
                            .accessibilityIdentifier("reset-all-shortcuts")
                    } label: {
                        SharedIcon(name: "more")
                    }.menuStyle(.button).buttonStyle(.borderless).menuIndicator(.hidden).fixedSize()
                        .accessibilityLabel("Keymap options").help("Keymap options").accessibilityIdentifier("keymap-menu")
                }
            }
            Section("Shortcuts") {
                ShortcutSearchField(store: store)
                HStack {
                    Picker("Kind of tool", selection: Binding(get: { page["context"].stableKey },
                        set: { key in
                            let choice = page["contexts"].array.first { $0["category"].stableKey == key }
                            action(["type": "shortcut_context", "category": choice?["category"].raw ?? NSNull()])
                        })) {
                        ForEach(page["contexts"].array, id: \.categoryKey) { choice in
                            Text(choice["label"].string).tag(choice["category"].stableKey)
                        }
                    }.labelsHidden().help("Show what shortcuts do with a kind of tool").accessibilityIdentifier("shortcut-context")
                    Picker("Actions", selection: Binding(get: { page["show"].string },
                        set: { action(["type": "shortcut_show", "show": $0]) })) {
                        ForEach(page["shows"].array, id: \.showKey) { choice in
                            Text(choice["label"].string).tag(choice["show"].string)
                        }
                    }.labelsHidden().help("Choose which actions to list").accessibilityIdentifier("shortcut-show")
                }
                if !page["filtering"].bool {
                    ForEach(page["categories"].array, id: \.shortcutID) { category in
                        ShortcutNavigationRow(label: category["id"].string, value: String(category["count"].uint)) {
                            action(["type": "shortcut_category", "id": category["id"].string])
                        }.accessibilityIdentifier("shortcut-category-" + category["id"].string)
                    }
                }
            }
            if !page["empty"].isNull {
                Section {
                    ContentUnavailableView(page["empty"]["title"].string, systemImage: "magnifyingglass",
                        description: Text(page["empty"]["description"].string))
                        .accessibilityIdentifier("shortcut-empty")
                }
            }
            if page["filtering"].bool {
                let modifiers = page["modifiers"].array.filter { $0["visible"].bool }
                if !modifiers.isEmpty {
                    Section("Modifier keys") { ModifierRows(store: store, modifiers: modifiers) }
                }
                ShortcutResults(store: store)
            }
        }.formStyle(.grouped)
    }
}

private struct CategoryPane: View {
    @ObservedObject var store: EditorStore
    private var page: JSON { store.snapshot["preferences"]["shortcut_page"] }
    var body: some View {
        PaneHeader(title: page["category"].string) {
            store.dispatch(["type": "preferences", "action": ["type": "shortcut_category", "id": NSNull()]])
        }
        Form {
            if page["category"].string == "Modifier keys" {
                Section {
                    ModifierRows(store: store, modifiers: page["modifiers"].array.filter { $0["visible"].bool })
                    Button {
                        store.dispatch(["type": "preferences", "action": ["type": "add_modifier_key"]])
                    } label: {
                        Label { Text("Add Modifier Key") } icon: { SharedIcon(name: "plus") }
                    }.accessibilityIdentifier("add-modifier-key")
                } footer: {
                    Text("Hold a key to use a tool or mode until you let go.")
                }
            }
            ShortcutResults(store: store)
        }.formStyle(.grouped)
    }
}

private struct ModifierRows: View {
    @ObservedObject var store: EditorStore
    let modifiers: [JSON]
    var body: some View {
        ForEach(modifiers, id: \.modifierKey) { modifier in
            ShortcutNavigationRow(label: modifier["label"].string, detail: modifier["detail"].string, value: modifier["action"].string) {
                store.dispatch(["type": "preferences", "action": ["type": "edit_modifier_key", "key": modifier["key"].raw]])
            }.accessibilityIdentifier("modifier-" + modifier["label"].string)
        }
    }
}

private struct ShortcutResults: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["preferences"] }
    private var groups: [(title: String, rows: [JSON])] {
        let page = model["shortcut_page"], nested = !page["category"].isNull && !page["filtering"].bool
        var groups: [(title: String, rows: [JSON])] = []
        for row in model["shortcuts"].array where row["visible"].bool {
            let title = row[nested ? "subgroup" : "group"].string
            if groups.last?.title == title { groups[groups.count - 1].rows.append(row) } else { groups.append((title, [row])) }
        }
        return groups
    }
    var body: some View {
        ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
            Section(group.title) {
                ForEach(group.rows, id: \.shortcutID) { row in ShortcutRow(store: store, row: row) }
            }
        }
    }
}

private struct ShortcutRow: View {
    @ObservedObject var store: EditorStore
    let row: JSON
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    private var subtitle: String {
        let scope = row["scope"].string
        let phrase = scope.isEmpty ? "" : scope == "Canvas" ? "On the canvas" : "With \(scope.lowercased()) tools"
        return [row["detail"].string, phrase].filter { !$0.isEmpty }.joined(separator: " · ")
    }
    var body: some View {
        HStack {
            Button { action(["type": "edit_shortcut", "id": row["id"].string]) } label: {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row["label"].string)
                        if !subtitle.isEmpty { Text(subtitle).font(.caption).foregroundStyle(.secondary) }
                    }
                    Spacer()
                    Text(row["shortcut"].string).foregroundStyle(.secondary)
                }.contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityIdentifier("shortcut-" + row["id"].string)
            if row["modified"].bool {
                Button { action(["type": "reset_shortcut", "id": row["id"].string]) } label: { SharedIcon(name: "reset") }
                    .buttonStyle(.borderless).accessibilityLabel("Reset to default").help("Reset to default")
                    .accessibilityIdentifier("shortcut-reset-" + row["id"].string)
            }
        }.background(NativePenScroll().frame(width: 0, height: 0))
    }
}

private struct ModifierKeyPane: View {
    @ObservedObject var store: EditorStore
    private var editor: JSON { store.snapshot["preferences"]["modifier_editor"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        PaneHeader(title: editor["label"].string) { action(["type": "close_modifier_key"]) }
        Form {
            Section {
                Toggle("Same for every tool", isOn: Binding(get: { !editor["per_tool"].bool },
                    set: { action(["type": "modifier_key_per_tool", "key": editor["key"].raw, "per_tool": !$0]) }))
                    .accessibilityIdentifier("modifier-same")
                ForEach(editor["actions"].array, id: \.categoryKey) { row in
                    ShortcutNavigationRow(label: row["label"].string, value: row["action"].string) {
                        action(["type": "open_modifier_picker", "key": editor["key"].raw, "category": row["category"].raw])
                    }.accessibilityIdentifier("modifier-action-" + (row["category"].isNull ? "all" : row["category"].string))
                }
            } header: {
                HStack {
                    Spacer()
                    if editor["modified"].bool {
                        Button("Reset to Default") { action(["type": "reset_modifier_key", "key": editor["key"].raw]) }
                            .accessibilityIdentifier("modifier-reset")
                    }
                }
            } footer: {
                Text("Hold \(editor["label"].string) to use an action until you let go.")
            }
            Section {
                Button(role: .destructive) { action(["type": "remove_modifier_key", "key": editor["key"].raw]) } label: {
                    Label { Text("Remove Modifier Key") } icon: { SharedIcon(name: "delete") }
                }.accessibilityIdentifier("modifier-remove")
            }
        }.formStyle(.grouped)
    }
}

struct GestureTriggerSections: View {
    @ObservedObject var store: EditorStore
    private var sections: [(title: String, rows: [JSON])] {
        var sections: [(title: String, rows: [JSON])] = []
        for row in store.snapshot["preferences"]["shortcut_page"]["triggers"].array {
            let title = row["section"].string
            if sections.last?.title == title { sections[sections.count - 1].rows.append(row) } else { sections.append((title, [row])) }
        }
        return sections
    }
    var body: some View {
        ForEach(sections, id: \.title) { section in
            Section(section.title) {
                ForEach(section.rows, id: \.shortcutID) { row in
                    ShortcutNavigationRow(label: row["label"].string, detail: row["detail"].string, value: row["action"].string) {
                        let pen = row["section"].string == "Pen buttons"
                        store.dispatch(["type": "preferences", "action": pen ? ["type": "edit_pen_button", "trigger": row["id"].string]
                            : ["type": "open_action_picker", "trigger": row["id"].string]])
                    }.accessibilityIdentifier("trigger-" + row["id"].string)
                }
            }
        }
    }
}

struct PenButtonPane: View {
    @ObservedObject var store: EditorStore
    private var editor: JSON { store.snapshot["preferences"]["pen_button_editor"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        VStack(spacing: 0) {
            PaneHeader(title: editor["label"].string) { action(["type": "close_pen_button"]) }
            Form {
                Section {
                    Toggle("Same for every tool", isOn: Binding(get: { !editor["per_tool"].bool },
                        set: { action(["type": "pen_button_per_tool", "trigger": editor["trigger"].raw, "per_tool": !$0]) }))
                        .accessibilityIdentifier("pen-button-same")
                    ForEach(editor["actions"].array, id: \.categoryKey) { row in
                        ShortcutNavigationRow(label: row["label"].string, value: row["action"].string) {
                            action(["type": "open_pen_button_picker", "trigger": editor["trigger"].raw, "category": row["category"].raw])
                        }.accessibilityIdentifier("pen-button-action-" + (row["category"].isNull ? "all" : row["category"].string))
                    }
                } header: {
                    HStack {
                        Spacer()
                        if editor["modified"].bool {
                            Button("Reset to Default") { action(["type": "reset_trigger", "trigger": editor["trigger"].raw]) }
                                .accessibilityIdentifier("pen-button-reset")
                        }
                    }
                } footer: {
                    Text("Tools, brushes and modes last while the button is held. Other actions run once.")
                }
            }.formStyle(.grouped)
        }
    }
}

private struct PaneHeader: View {
    let title: String
    let back: () -> Void
    var body: some View {
        HStack(spacing: 8) {
            Button(action: back) { SharedIcon(name: "go-previous").contentShape(Rectangle()) }
                .buttonStyle(.borderless).accessibilityLabel("Back").help("Back").accessibilityIdentifier("settings-back")
            Text(title).font(.headline).accessibilityIdentifier("shortcut-pane-title")
            Spacer()
        }.padding(.horizontal, 20).padding(.top, 12)
    }
}

struct ShortcutNavigationRow: View {
    let label: String
    var detail = ""
    let value: String
    let open: () -> Void
    var body: some View {
        Button(action: open) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(label)
                    if !detail.isEmpty { Text(detail).font(.caption).foregroundStyle(.secondary) }
                }
                Spacer()
                Text(value).foregroundStyle(.secondary)
                SharedIcon(name: "go-next", size: 12).foregroundStyle(.secondary)
            }.contentShape(Rectangle())
        }.buttonStyle(.plain).background(NativePenScroll().frame(width: 0, height: 0))
    }
}

private struct ShortcutSearchField: View {
    @ObservedObject var store: EditorStore
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        EditorTextField("Search or press a shortcut", value: store.snapshot["preferences"]["shortcut_query"].string) {
            action(["type": "search_shortcuts", "query": $0])
        }
            .editorSearchInput().textFieldStyle(.roundedBorder).accessibilityIdentifier("shortcut-search")
            .onKeyPress(phases: .down) { press in
                guard let chord = ShortcutSearchChord(press) else { return .ignored }
                action(["type": "search_shortcut_key", "chord": chord.raw])
                return .handled
            }
    }
}

private struct ShortcutSearchChord {
    let raw: [String: Any]
    init?(_ press: KeyPress) {
        let command = !press.modifiers.intersection([.command, .control]).isEmpty, alt = press.modifiers.contains(.option)
        let names: [KeyEquivalent: String] = [.upArrow: "arrowup", .downArrow: "arrowdown", .leftArrow: "arrowleft",
            .rightArrow: "arrowright", .home: "home", .end: "end", .pageUp: "pageup", .pageDown: "pagedown",
            .delete: "backspace", .deleteForward: "delete", .return: "enter", .tab: "tab", .space: " ", .escape: "escape"]
        let scalar = press.characters.unicodeScalars.first?.value ?? 0
        let function = (0xF704...0xF71B).contains(scalar) ? "f\(scalar - 0xF703)" : nil
        let key = names[press.key] ?? function ?? press.characters.lowercased()
        let editing = press.modifiers == .command
            && ["a", "c", "v", "x", "z", "backspace", "delete", "arrowleft", "arrowright", "home", "end"].contains(key)
        guard !key.isEmpty, !editing, command || alt || function != nil else { return nil }
        raw = ["key": key, "command": command, "shift": press.modifiers.contains(.shift), "alt": alt]
    }
}

private struct RecordingRow: View {
    @ObservedObject var store: EditorStore
    let capture: JSON
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        HStack(spacing: 10) {
            SharedIcon(name: capture["existing"].bool || !capture["notice"].string.isEmpty ? "info" : "keyboard")
                .foregroundStyle(!capture["notice"].string.isEmpty && !capture["existing"].bool ? Color.orange : Color.primary)
            VStack(alignment: .leading, spacing: 2) {
                Text(capture["shortcut"].string).accessibilityIdentifier("shortcut-captured")
                if !capture["notice"].string.isEmpty {
                    Text(capture["notice"].string).font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
            Button("Cancel") { action(["type": "cancel_shortcut"]) }.accessibilityIdentifier("shortcut-cancel")
            Button(capture["existing"].bool ? "Open" : capture["conflict"].isNull ? "Add" : "Reassign") {
                action(["type": "confirm_shortcut", "replace": !capture["conflict"].isNull])
            }.buttonStyle(.borderedProminent).disabled(capture["chord"].isNull || !capture["error"].isNull)
                .accessibilityIdentifier("shortcut-confirm")
        }.background(ShortcutKeyCapture { key, command, shift, alt in
            store.captureShortcut(key: key, command: command, shift: shift, alt: alt)
        }.frame(width: 1, height: 1))
    }
}

private struct ShortcutEditorSheet: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["preferences"] }
    private var editor: JSON { model["shortcut_editor"] }
    private var capture: JSON { model["capture"]["id"].string == editor["id"].string ? model["capture"] : JSON() }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        NavigationStack {
            Form {
                if !editor["description"].string.isEmpty {
                    Text(editor["description"].string).foregroundStyle(.secondary)
                }
                if capture.isNull && !model["error"].isNull {
                    Text(model["error"].string).foregroundStyle(.red)
                }
                Section {
                    ForEach(editor["bindings"].array.indices, id: \.self) { index in
                        HStack {
                            Text(editor["bindings"][index].string)
                            Spacer()
                            Button { action(["type": "remove_shortcut", "id": editor["id"].string, "index": index]) } label: {
                                SharedIcon(name: "delete")
                            }.buttonStyle(.borderless).accessibilityLabel("Remove shortcut")
                                .accessibilityIdentifier("shortcut-remove-\(index)")
                        }
                    }
                    if !capture.isNull {
                        RecordingRow(store: store, capture: capture)
                    } else if editor["can_add"].bool {
                        Button {
                            action(["type": "begin_shortcut", "id": editor["id"].string])
                        } label: {
                            Label { Text("Add Shortcut") } icon: { SharedIcon(name: "plus") }
                        }.accessibilityIdentifier("shortcut-add")
                    }
                } header: {
                    HStack {
                        Spacer()
                        if editor["modified"].bool {
                            Button("Reset to Default") { action(["type": "reset_shortcut", "id": editor["id"].string]) }
                                .accessibilityIdentifier("shortcut-reset")
                        }
                    }
                } footer: {
                    Text((["Default: " + (editor["defaults"].array.isEmpty ? "none" : editor["defaults"].array.map(\.string).joined(separator: " / "))]
                        + editor["overlaps"].array.map(\.string)).joined(separator: "\n"))
                }
                if !editor["gestures"].array.isEmpty {
                    Section {
                        ForEach(editor["gestures"].array.indices, id: \.self) { index in Text(editor["gestures"][index].string) }
                    } header: { Text("Pen and Touch") } footer: { Text("Change these on the Pen & Input page.") }
                }
            }.formStyle(.grouped)
                .navigationTitle(editor["label"].string)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { action(["type": capture.isNull ? "close_shortcut_editor" : "cancel_shortcut"]) }
                            .accessibilityIdentifier("shortcut-editor-done")
                    }
                }
        }.frame(minWidth: 360, idealWidth: 460, minHeight: 320)
    }
}

private struct ModifierCaptureSheet: View {
    @ObservedObject var store: EditorStore
    private var capture: JSON { store.snapshot["preferences"]["capture"] }
    var body: some View {
        NavigationStack {
            Form {
                Text("Press the key or button to hold.").foregroundStyle(.secondary)
                if !capture.isNull { Section { RecordingRow(store: store, capture: capture) } }
            }.formStyle(.grouped).navigationTitle("New Modifier Key")
        }.frame(minWidth: 360, idealWidth: 420, minHeight: 220)
    }
}

struct ActionPickerSheet: View {
    @ObservedObject var store: EditorStore
    private var picker: JSON { store.snapshot["preferences"]["shortcut_page"]["picker"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    private var offersNothing: Bool {
        let query = picker["query"].string.trimmingCharacters(in: .whitespaces).lowercased()
        return query.isEmpty || "nothing".contains(query)
    }
    var body: some View {
        NavigationStack {
            Form {
                if !picker["description"].string.isEmpty {
                    Text(picker["description"].string).foregroundStyle(.secondary).accessibilityIdentifier("action-picker-description")
                }
                EditorTextField("Search actions", value: picker["query"].string) {
                    action(["type": "search_action_picker", "query": $0])
                }.editorSearchInput().textFieldStyle(.roundedBorder).accessibilityIdentifier("action-picker-search")
                if offersNothing {
                    Section { choice(id: "", label: "Nothing", detail: "", selected: picker["nothing"].bool) }
                }
                ForEach(picker["sections"].array.indices, id: \.self) { index in
                    let section = picker["sections"][index]
                    Section(section["title"].string) {
                        ForEach(section["actions"].array, id: \.shortcutID) { item in
                            choice(id: item["id"].string, label: item["label"].string, detail: item["detail"].string, selected: item["selected"].bool)
                        }
                    }
                }
                if !offersNothing && picker["sections"].array.isEmpty {
                    ContentUnavailableView("No Results Found", systemImage: "magnifyingglass", description: Text("Try a different search."))
                }
            }.formStyle(.grouped)
                .navigationTitle(picker["title"].string)
                .toolbar {
                    if picker["modified"].bool {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Reset") { action(["type": "reset_trigger", "trigger": picker["trigger"].string]) }
                                .accessibilityIdentifier("action-picker-reset")
                        }
                    }
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { action(["type": "close_action_picker"]) }.accessibilityIdentifier("action-picker-close")
                    }
                }
        }.frame(minWidth: 380, idealWidth: 460, minHeight: 420)
    }
    private func choice(id: String, label: String, detail: String, selected: Bool) -> some View {
        Button { action(["type": "choose_action", "id": id]) } label: {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(label)
                    if !detail.isEmpty { Text(detail).font(.caption).foregroundStyle(.secondary) }
                }
                Spacer()
                SharedIcon(name: "check").opacity(selected ? 1 : 0)
            }.contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityIdentifier("action-" + (id.isEmpty ? "nothing" : id))
    }
}

private struct KeymapDetailsSheet: View {
    @ObservedObject var store: EditorStore
    private var keymap: JSON { store.snapshot["preferences"]["keymap"] }
    var body: some View {
        NavigationStack {
            Form {
                Text(keymap["source"].string).foregroundStyle(.secondary)
                if !keymap["links"].array.isEmpty {
                    Section {
                        ForEach(keymap["links"].array.indices, id: \.self) { index in
                            let link = keymap["links"][index].string
                            if let url = URL(string: link) { Link(Self.name(link), destination: url) }
                        }
                    }
                }
                Section {
                    if keymap["differences"].array.isEmpty {
                        difference("No differences", "This keymap uses the CapyCanvas defaults.")
                    }
                    ForEach(keymap["differences"].array.indices, id: \.self) { index in
                        difference(keymap["differences"][index]["trigger"].string, keymap["differences"][index]["note"].string)
                    }
                }
            }.formStyle(.grouped)
                .navigationTitle(keymap["title"].string)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") {
                            store.dispatch(["type": "preferences", "action": ["type": "keymap_details", "open": false]])
                        }.accessibilityIdentifier("keymap-details-done")
                    }
                }
        }.frame(minWidth: 380, idealWidth: 480, minHeight: 360)
    }
    private func difference(_ trigger: String, _ note: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(trigger)
            Text(note).font(.caption).foregroundStyle(.secondary)
        }
    }
    private static func name(_ link: String) -> String {
        let last = link.split(separator: "/").last.map(String.init) ?? link
        return (last.split(separator: ".").first.map(String.init) ?? last).replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: "-", with: " ")
    }
}

private struct KeymapImportSheet: View {
    @ObservedObject var store: EditorStore
    private var preview: JSON { store.snapshot["preferences"]["keymap"]["import"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    private var sections: [(String, [JSON])] {
        [("Added", preview["added"].array), ("Changed", preview["changed"].array), ("Removed", preview["removed"].array),
         ("Not available", preview["unavailable"].array)].filter { !$0.1.isEmpty }
    }
    var body: some View {
        NavigationStack {
            Form {
                if sections.isEmpty { Text("No shortcuts change.") }
                ForEach(sections, id: \.0) { title, items in
                    Section("\(title) (\(items.count))") {
                        ForEach(items.indices, id: \.self) { index in Text(items[index].string) }
                    }
                }
            }.formStyle(.grouped)
                .navigationTitle("Import \(preview["title"].string)?")
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { action(["type": "cancel_keymap_import"]) }.accessibilityIdentifier("cancel-keymap-import")
                    }
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Import") { action(["type": "confirm_keymap_import"]) }.accessibilityIdentifier("confirm-keymap-import")
                    }
                }
        }.frame(minWidth: 380, idealWidth: 460, minHeight: 320)
    }
}

struct KeymapDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.capyKeymap, .json] }
    let text: String
    init(text: String) { self.text = text }
    init(configuration: ReadConfiguration) throws {
        text = String(data: configuration.file.regularFileContents ?? Data(), encoding: .utf8) ?? ""
    }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: Data(text.utf8)) }
}

extension UTType {
    static let capyKeymap = UTType(filenameExtension: "capykeys", conformingTo: .json) ?? .json
}

private extension JSON {
    var shortcutID: String { self["id"].string }
    var categoryKey: String { self["category"].stableKey }
    var showKey: String { self["show"].string }
    var modifierKey: String { self["key"].stableKey }
}
