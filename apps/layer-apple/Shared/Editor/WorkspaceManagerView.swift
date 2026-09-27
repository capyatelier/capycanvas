import SwiftUI

struct WorkspaceManagerPresentation: ViewModifier {
    @ObservedObject var workspaces: WorkspaceController
    func body(content: Content) -> some View {
        content.allowsHitTesting(workspaces.ready && !workspaces.busy && !workspaces.readOnly)
            .overlay(alignment: .bottom) {
                if !workspaces.presented && (workspaces.error != nil || workspaces.readOnly || !workspaces.ready) {
                    VStack(alignment: .leading, spacing: 8) {
                        if let error = workspaces.error { Text(error).textSelection(.enabled) }
                        else if workspaces.view["owner_lost"].bool { Text("Workspace ownership needs recovery.") }
                        else if workspaces.readOnly { Text("Saving workspace…") }
                        else { HStack { ProgressView().controlSize(.small); Text("Opening workspace…") } }
                        if workspaces.error != nil || workspaces.view["owner_lost"].bool {
                            HStack {
                                Button("Retry") { workspaces.send(["type": "retry"]) }
                                if workspaces.ready {
                                    Button("Save as New Workspace…") { workspaces.form(["type": "save_as_new"]) }
                                }
                            }
                        }
                    }.padding(14).frame(maxWidth: 580, alignment: .leading)
                        .modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 12))).padding(12)
                }
            }
            .sheet(isPresented: Binding(get: { workspaces.presented }, set: { if !$0 { workspaces.dismiss() } })) {
                WorkspaceManagerView(workspaces: workspaces)
                    .modifier(EditorPopupPresentation())
            }
    }
}

struct WorkspaceManagerView: View {
    @ObservedObject var workspaces: WorkspaceController
    @State private var query = ""
    private var view: JSON { workspaces.view }
    private var toolbarMode: Bool { ["this_workspace", "toolbar_library"].contains(workspaces.page) }
    private var message: String? {
        if !view["error"].isNull { return view["error"].string }
        return view["switcher_error"].isNull ? nil : view["switcher_error"].string
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                Text(view["prompt"]["title"].isNull ? view["title"].string : view["prompt"]["title"].string)
                    .font(.title2).fontWeight(.semibold)
                Spacer()
                if view["prompt"].isNull && workspaces.page == "workspaces" {
                    Button {
                        workspaces.form(["type": "new"])
                    } label: { SharedIcon(name: "plus").frame(width: 24, height: 24) }
                        .accessibilityLabel("New Workspace")
                        .accessibilityIdentifier("workspace-action-new")
                        .disabled(workspaces.busy)
                } else if view["prompt"].isNull && toolbarMode {
                    Button("Close", role: .cancel) { workspaces.dismiss() }.keyboardShortcut(.cancelAction)
                        .disabled(workspaces.busy).accessibilityIdentifier("workspace-manager-close")
                }
            }
            if !view["prompt"].isNull { WorkspaceManagerForm(workspaces: workspaces, spec: view["prompt"]) }
            else {
                if let error = message {
                    Text(error).foregroundStyle(.red).textSelection(.enabled).accessibilityIdentifier("workspace-manager-error")
                }
                if workspaces.page == "history" { history } else { browser }
            }
        }.padding(20).frame(minWidth: 340, idealWidth: workspaces.page == "history" ? 480 : 620, maxWidth: 800,
            minHeight: 360, idealHeight: 600, maxHeight: 900)
            .interactiveDismissDisabled(workspaces.busy)
            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-library-manager")
            .onChange(of: workspaces.page) { _, _ in query = "" }
    }
    private var browser: some View {
        VStack(alignment: .leading, spacing: 14) {
            if toolbarMode {
                HStack {
                    ForEach([("this_workspace", "This Workspace"), ("toolbar_library", "Saved Toolbars")], id: \.0) { page in
                        Button(page.1) { workspaces.open(page.0) }
                            .tint(workspaces.page == page.0 ? .accentColor : .secondary)
                    }
                }
            } else if !view["intro"].string.isEmpty {
                Text(view["intro"].string)
                    .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            if toolbarMode { HStack {
                TextField("Search", text: $query).editorSearchInput().textFieldStyle(.roundedBorder)
                    .onChange(of: query) { _, value in workspaces.search(value) }.accessibilityIdentifier("workspace-manager-search")
                if workspaces.page == "this_workspace" {
                    Button("New Toolbar…") { workspaces.form(["type": "new_toolbar", "value": NSNull()]) }
                        .accessibilityIdentifier("workspace-action-new_toolbar")
                }
            } }
            if workspaces.page == "workspaces" {
                WorkspaceSwitcherRows(workspaces: workspaces)
            } else { EditorScrollView {
                LazyVStack(spacing: 8) {
                    if view["rows"].array.isEmpty { Text("No items found.").foregroundStyle(.secondary).padding(20) }
                    ForEach(view["rows"].array, id: \.managerID) { row in
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 14) { selectableRow(row); rowActions(row) }
                            VStack(alignment: .leading, spacing: 10) { selectableRow(row); rowActions(row) }
                        }.padding(12).background(view["selected"].string == row["id"].string ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 10))
                            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-item-" + row["id"].string)
                    }
                }
            } }
            if !toolbarMode {
                HStack(spacing: 8) {
                    Button("Cancel", role: .cancel) { workspaces.dismiss() }.keyboardShortcut(.cancelAction)
                        .accessibilityIdentifier("workspace-manager-close")
                        .buttonStyle(WorkspaceManagerButtonStyle())
                    Button(view["primary"].string) { workspaces.send(["type": "confirm"]) }
                        .buttonStyle(WorkspaceManagerButtonStyle(primary: true))
                        .disabled(!view["enabled"].bool || workspaces.busy)
                        .accessibilityIdentifier("workspace-manager-apply")
                }
            }
            if workspaces.busy || view["loading"].bool { ProgressView().controlSize(.small) }
        }.disabled(workspaces.busy)
    }
    private func selectableRow(_ row: JSON) -> some View {
        Button { workspaces.select(row["id"].string) } label: { rowLabel(row).contentShape(Rectangle()) }
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
            ForEach(row["actions"].array.filter { $0["primary"].bool }, id: \.managerActionID) { button in action(button) }
            let secondary = row["actions"].array.filter { !$0["primary"].bool }
            if !secondary.isEmpty {
                EditorMenuButton(menu: {
                    AppleContextMenu(JSON(["sections": [secondary.map(\.raw)]])) { workspaces.activate($0) }
                }) { SharedIcon(name: "more").frame(width: 20) }
                    .accessibilityLabel("Actions for " + row["title"].string)
            }
        }.fixedSize(horizontal: true, vertical: false)
    }
    private func action(_ button: JSON) -> some View {
        Button(button["label"].string, role: button["action"]["type"].string.hasPrefix("delete") ? .destructive : nil) {
            workspaces.activate(button["action"])
        }.disabled(!button["enabled"].bool).accessibilityIdentifier("workspace-action-" + button["action"]["type"].string)
    }
    private var history: some View {
        VStack(alignment: .leading, spacing: 14) {
            EditorScrollView {
                LazyVStack(spacing: 6) {
                    ForEach(view["rows"].array, id: \.managerID) { row in
                        Button { workspaces.select(row["id"].string) } label: {
                            rowLabel(row).padding(12).contentShape(Rectangle())
                        }.buttonStyle(EditorControlButtonStyle(selected: view["selected"].string == row["id"].string))
                            .accessibilityAddTraits(view["selected"].string == row["id"].string ? .isSelected : [])
                            .accessibilityIdentifier("workspace-history-" + row["id"].string)
                    }
                }
            }.disabled(workspaces.busy)
            HStack {
                Button("Cancel", role: .cancel) { workspaces.dismiss() }.keyboardShortcut(.cancelAction)
                    .buttonStyle(WorkspaceManagerButtonStyle())
                Button(view["primary"].string) { workspaces.send(["type": "confirm"]) }
                    .buttonStyle(WorkspaceManagerButtonStyle(primary: true))
                    .disabled(workspaces.busy || !view["enabled"].bool)
                    .accessibilityIdentifier("workspace-history-restore")
            }
        }
    }
}

/// Solid, equally sized footer actions matching the shared workspace manager.
private struct WorkspaceManagerButtonStyle: ButtonStyle {
    var primary = false
    @Environment(\.isEnabled) private var enabled
    @Environment(\.editorPopupStore) private var store
    private var foreground: Color {
        guard let source = store?.state["palette"], !source["text"].isNull else { return .primary }
        return EditorPalette(source: source)["text"]
    }
    private var palette: EditorPalette { EditorPalette(source: store?.state["palette"] ?? JSON()) }
    func makeBody(configuration: Configuration) -> some View {
        let prominent = primary && enabled
        configuration.label.fontWeight(.semibold).lineLimit(2).multilineTextAlignment(.center)
            .padding(.horizontal, 16).frame(maxWidth: .infinity, minHeight: 40)
            .foregroundStyle(prominent ? palette.accentForeground : foreground)
            .background(prominent ? palette.accent : foreground.opacity(0.10))
            .overlay { if configuration.isPressed { Color.black.opacity(0.12) } }
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .overlay { RoundedRectangle(cornerRadius: 6).strokeBorder(foreground.opacity(prominent ? 0 : 0.10)) }
            .opacity(enabled ? 1 : 0.55).contentShape(RoundedRectangle(cornerRadius: 6))
    }
}

private struct WorkspaceManagerForm: View {
    @ObservedObject var workspaces: WorkspaceController
    let spec: JSON
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(spec["message"].string).fixedSize(horizontal: false, vertical: true)
            if !spec["name"].isNull {
                TextField("Name", text: $workspaces.formName).textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("workspace-form-name")
            }
            if !spec["description"].isNull {
                TextField("Description (optional)", text: $workspaces.formDescription, axis: .vertical)
                    .textFieldStyle(.roundedBorder).lineLimit(3...6).accessibilityIdentifier("workspace-form-description")
            }
            if !spec["choices"].array.isEmpty {
                Picker(spec["choice_label"].string, selection: $workspaces.formChoice) {
                    ForEach(spec["choices"].array, id: \.managerID) { choice in Text(choice["label"].string).tag(choice["id"].string) }
                }.accessibilityIdentifier("workspace-form-choice")
            }
            if let error = workspaces.error { Text(error).foregroundStyle(.red).accessibilityIdentifier("workspace-form-error") }
            Spacer(minLength: 16)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { workspaces.cancelPrompt() }.keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("workspace-form-cancel")
                Button(spec["confirm"].string, role: spec["destructive"].bool ? .destructive : nil) { workspaces.submit() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(workspaces.busy || !spec["name"].isNull && workspaces.formName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("workspace-form-confirm")
            }
        }.accessibilityIdentifier("workspace-library-form")
    }
}

private extension JSON {
    var managerID: String { self["id"].string }
    var managerActionID: String { self["action"].stableKey }
}
