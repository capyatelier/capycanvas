import SwiftUI

struct WorkspaceManagerPresentation: ViewModifier {
    @ObservedObject var manager: WorkspaceManager
    @ObservedObject var library: WorkspaceLibrary
    func body(content: Content) -> some View {
        content.allowsHitTesting(library.ready && !library.busy && !library.readOnly)
            .overlay(alignment: .bottom) {
                if !manager.presented && (library.error != nil || library.readOnly || !library.ready) {
                    VStack(alignment: .leading, spacing: 8) {
                        if let error = library.error { Text(error).textSelection(.enabled) }
                        else if library.readOnly { Text("Workspace ownership needs recovery.") }
                        else { HStack { ProgressView().controlSize(.small); Text("Opening workspace…") } }
                        if library.error != nil || library.readOnly {
                            HStack {
                                Button("Retry") { manager.activate(JSON(["type": "retry_storage"])) }
                                if library.ready {
                                    Button("Save as New Workspace…") { manager.activate(JSON(["type": "save_as_new"])) }
                                }
                            }
                        }
                    }.padding(14).frame(maxWidth: 580, alignment: .leading)
                        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12)).padding(12)
                }
            }
            .sheet(isPresented: $manager.presented, onDismiss: { manager.dismissed() }) {
                WorkspaceManagerView(manager: manager, library: library)
                    .modifier(WorkspacePackagePicker(files: manager.files))
            }
    }
}

struct WorkspaceManagerView: View {
    @ObservedObject var manager: WorkspaceManager
    @ObservedObject var library: WorkspaceLibrary
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                Text(manager.prompt?["title"].string ?? (manager.history.isNull ? manager.title : manager.history["title"].string))
                    .font(.title2).fontWeight(.semibold)
                Spacer()
                if manager.prompt == nil && manager.history.isNull && !manager.toolbarMode {
                    Button {
                        manager.activate(manager.page == "templates"
                            ? JSON(["type": "save_as_template", "value": library.status["active_id"].raw]) : JSON(["type": "new"]))
                    } label: { Image(systemName: "plus").frame(width: 24, height: 24) }
                        .accessibilityLabel(manager.page == "templates" ? "Save Layout" : "New Workspace")
                        .accessibilityIdentifier(manager.page == "templates" ? "workspace-action-save_as_template" : "workspace-action-new")
                        .disabled(manager.processing)
                } else if manager.prompt == nil && manager.history.isNull {
                    Button("Close", role: .cancel) { manager.presented = false }.keyboardShortcut(.cancelAction)
                        .disabled(manager.processing).accessibilityIdentifier("workspace-manager-close")
                }
            }
            if let prompt = manager.prompt { WorkspaceManagerForm(manager: manager, spec: prompt) }
            else {
                if let error = manager.error ?? library.error {
                    Text(error).foregroundStyle(.red).textSelection(.enabled).accessibilityIdentifier("workspace-manager-error")
                }
                if manager.history.isNull { browser }
                else { history }
            }
        }.padding(20).frame(minWidth: 340, idealWidth: manager.history.isNull ? 620 : 480, maxWidth: 800,
            minHeight: 360, idealHeight: 600, maxHeight: 900)
            .interactiveDismissDisabled(manager.processing || library.busy)
            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-library-manager")
    }
    private var browser: some View {
        VStack(alignment: .leading, spacing: 14) {
            if manager.toolbarMode {
                HStack {
                    ForEach(manager.catalog["pages"].array.filter { ["this_workspace", "toolbar_library"].contains($0["id"].string) }, id: \.managerID) { page in
                        Button(page["label"].string) {
                            Task { do { try await manager.show(page["id"].string) } catch { manager.error = error.localizedDescription } }
                        }.tint(manager.page == page["id"].string ? .accentColor : .secondary)
                    }
                }
            } else {
                Text(manager.catalog[manager.page == "templates" ? "template_description" : "description"].string)
                    .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                TextField("Search", text: $manager.query).textFieldStyle(.roundedBorder)
                    .onChange(of: manager.query) { _, _ in manager.search() }.accessibilityIdentifier("workspace-manager-search")
                if manager.page == "this_workspace" {
                    Button("New Toolbar…") { manager.activate(JSON(["type": "new_toolbar"])) }.accessibilityIdentifier("workspace-action-new_toolbar")
                }
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    if manager.view["rows"].array.isEmpty { Text("No items found.").foregroundStyle(.secondary).padding(20) }
                    ForEach(manager.view["rows"].array, id: \.managerID) { row in
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 14) { selectableRow(row); rowActions(row) }
                            VStack(alignment: .leading, spacing: 10) { selectableRow(row); rowActions(row) }
                        }.padding(12).background(manager.selection == row["id"].string ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 10))
                            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-item-" + row["id"].string)
                    }
                }
            }
            if !manager.toolbarMode {
                HStack {
                    Spacer()
                    Button("Cancel", role: .cancel) { manager.presented = false }.keyboardShortcut(.cancelAction)
                        .accessibilityIdentifier("workspace-manager-close")
                    if manager.view["details"].isNull {
                        Button(manager.catalog[manager.page == "templates" ? "load_label" : "switch_label"].string) { }
                            .disabled(true)
                    }
                    ForEach(manager.view["details"]["actions"].array.filter { $0["primary"].bool }, id: \.managerActionID) { button in action(button) }
                        .disabled(manager.selecting)
                }
            }
            if manager.processing { ProgressView().controlSize(.small) }
        }.disabled(manager.processing)
    }
    private func selectableRow(_ row: JSON) -> some View {
        Button { manager.select(row["id"].string) } label: { rowLabel(row).contentShape(Rectangle()) }
            .buttonStyle(.plain).accessibilityIdentifier("workspace-select-" + row["id"].string)
    }
    private func rowLabel(_ row: JSON) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(row["title"].string).fontWeight(.medium).fixedSize(horizontal: false, vertical: true)
            if !row["subtitle"].string.isEmpty { Text(row["subtitle"].string).font(.caption).foregroundStyle(.secondary) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func rowActions(_ row: JSON) -> some View {
        HStack {
            ForEach(row["actions"].array.filter { manager.toolbarMode && $0["primary"].bool }, id: \.managerActionID) { button in action(button) }
            let secondary = row["actions"].array.filter { !$0["primary"].bool }
            if !secondary.isEmpty {
                Menu { ForEach(secondary, id: \.managerActionID) { button in action(button) } }
                    label: { Image(systemName: "ellipsis").frame(width: 20) }
                    .accessibilityLabel("Actions for " + row["title"].string)
            }
        }.fixedSize(horizontal: true, vertical: false)
    }
    private func action(_ button: JSON) -> some View {
        Button(button["label"].string, role: button["action"]["type"].string.hasPrefix("delete") ? .destructive : nil) {
            manager.activate(button["action"])
        }.disabled(!button["enabled"].bool).accessibilityIdentifier("workspace-action-" + button["action"]["type"].string)
    }
    private var history: some View {
        VStack(alignment: .leading, spacing: 14) {
            ScrollView {
                LazyVStack(spacing: 6) {
                    ForEach(manager.history["rows"].array, id: \.managerID) { row in
                        Button { manager.selectHistory(row["id"].string) } label: {
                            HStack {
                                rowLabel(row)
                                if manager.history["selected"].string == row["id"].string { Image(systemName: "checkmark") }
                            }.padding(12).contentShape(Rectangle())
                                .background(Color.accentColor.opacity(manager.history["selected"].string == row["id"].string ? 0.14 : 0), in: RoundedRectangle(cornerRadius: 10))
                        }.buttonStyle(.plain).accessibilityIdentifier("workspace-history-" + row["id"].string)
                    }
                }
            }.disabled(manager.processing)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { manager.presented = false }.keyboardShortcut(.cancelAction)
                Button("Restore This Version") { manager.historyAction(open: false) }
                    .disabled(manager.processing || !library.previewingLayout || manager.history["restore"].isNull)
                    .accessibilityIdentifier("workspace-history-restore")
            }
        }
    }
}

private struct WorkspaceManagerForm: View {
    @ObservedObject var manager: WorkspaceManager
    let spec: JSON
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(spec["message"].string).fixedSize(horizontal: false, vertical: true)
            if !spec["name"].isNull {
                TextField("Name", text: $manager.formName).textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("workspace-form-name")
            }
            if !spec["description"].isNull {
                TextField("Description (optional)", text: $manager.formDescription, axis: .vertical)
                    .textFieldStyle(.roundedBorder).lineLimit(3...6).accessibilityIdentifier("workspace-form-description")
            }
            if !spec["choices"].array.isEmpty {
                Picker(spec["choice_label"].string, selection: $manager.formChoice) {
                    ForEach(spec["choices"].array, id: \.managerID) { choice in Text(choice["label"].string).tag(choice["id"].string) }
                }.accessibilityIdentifier("workspace-form-choice")
            }
            if let error = manager.formError { Text(error).foregroundStyle(.red).accessibilityIdentifier("workspace-form-error") }
            Spacer(minLength: 16)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { manager.answer(confirm: false) }.keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("workspace-form-cancel")
                Button(spec["confirm"].string, role: spec["destructive"].bool ? .destructive : nil) { manager.answer(confirm: true) }
                    .keyboardShortcut(.defaultAction).disabled(!spec["name"].isNull && manager.formName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("workspace-form-confirm")
            }
        }.accessibilityIdentifier("workspace-library-form")
    }
}

private extension JSON {
    var managerID: String { self["id"].string }
    var managerActionID: String { self["action"].stableKey }
}
