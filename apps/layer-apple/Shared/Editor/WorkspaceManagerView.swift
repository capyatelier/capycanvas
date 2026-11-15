import SwiftUI

struct WorkspaceManagerPresentation: ViewModifier {
    @ObservedObject var workspaces: WorkspaceController
    func body(content: Content) -> some View {
        if let store = workspaces.store {
            content.editorSheet(store: store, intent: workspaces.view["prompt"].isNull || workspaces.inlineRename
                ? workspaces.view["dialog"] : workspaces.view["prompt"]["dialog"],
                isPresented: Binding(get: { workspaces.presented }, set: { if !$0 { workspaces.dismiss() } })) {
                WorkspaceManagerView(workspaces: workspaces)
                    .modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
            }
        } else { content }
    }
}

struct WorkspaceStartupNotice: View {
    @ObservedObject var workspaces: WorkspaceController
    let minTextWidth: CGFloat
    @Environment(\.capyCommonCopy) private var common
    @Environment(\.capyNativeCopy) private var copy
    var body: some View {
        let status = workspaces.view["status"], action = status["action"]
        if !workspaces.presented && (workspaces.visibleError != nil
            || workspaces.error == nil && (workspaces.readOnly || !workspaces.ready || !action.isNull)) {
            EditorNotice(message: workspaces.visibleError
                ?? (workspaces.view["owner_lost"].bool ? copy["header"]["owner_lost"].string
                    : workspaces.readOnly ? (workspaces.store?.bootstrap["recovery"]["saving_workspace"].string ?? "")
                    : workspaces.ready ? status["text"].string
                    : (workspaces.store?.bootstrap["recovery"]["opening_workspace"].string ?? "")),
                minTextWidth: minTextWidth, working: workspaces.busy || workspaces.error == nil && !workspaces.ready) {
                if status["retry"].bool {
                    Button(copy["header"]["retry"].string) { workspaces.send(["type": "retry"]) }.disabled(workspaces.busy)
                }
                if !action.isNull {
                    Button(action["label"].string) { workspaces.send(["type": "form", "action": action["action"].raw]) }
                        .disabled(workspaces.busy || !action["enabled"].bool).accessibilityIdentifier("workspace-status-action")
                }
                if workspaces.error != nil && workspaces.ready && !workspaces.readOnly {
                    Button(common["ok"].string) { workspaces.dismissError() }
                }
            }
        }
    }
}

struct WorkspaceInteraction: ViewModifier {
    @ObservedObject var workspaces: WorkspaceController
    func body(content: Content) -> some View {
        content.allowsHitTesting(workspaces.ready && !workspaces.busy && !workspaces.readOnly)
    }
}

struct WorkspaceManagerView: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.capyCommonCopy) private var common
    @Environment(\.capyInterfaceLanguage) private var language
    @Environment(\.editorDialogPage) private var page
    @ObservedObject var workspaces: WorkspaceController
    @State private var query = ""
    @FocusState private var searchFocused: Bool
    @State private var workspaceRowHeights: [String: CGFloat] = [:]
    private var header: JSON { nativeCopy["header"] }
    private var view: JSON { workspaces.view }
    private var toolbarMode: Bool { workspaces.toolbarMode }
    private var singlePane: Bool { workspaces.dialogLayout["single_pane"].bool }
    private var subpage: Bool { workspaces.dialogLayout["subpage"].bool }
    private var workspaceRowsHeight: CGFloat {
        view["rows"].array.prefix(7).reduce(0) { height, row in
            height + max(56, workspaceRowHeights[row["id"].string] ?? 56)
        }
    }
    private var message: String? {
        if !view["error"].isNull { return view["error"].string }
        return view["switcher_error"].isNull ? nil : view["switcher_error"].string
    }
    var body: some View {
        Group {
            if view["prompt"]["dialog"]["role"].string == "alert" || page && !view["prompt"].isNull && !workspaces.inlineRename {
                WorkspaceManagerForm(workspaces: workspaces, spec: view["prompt"])
            } else { manager }
        }
            .onNativeTextKey(.escape) { escape(); return .handled }
            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-library-manager")
    }
    private var manager: some View {
        VStack(alignment: .leading, spacing: page ? 0 : 12) {
            if page {
                EditorPageHeader(intent: view["dialog"], title: subpage ? view["details"]["title"].string : view["title"].string,
                    identifier: "workspace-manager", subpage: subpage, primaryLabel: view["primary"].string,
                    primaryEnabled: view["enabled"].bool && (!workspaces.busy || workspaces.inlineRename),
                    primaryIdentifier: workspaces.page == "history" ? "workspace-history-restore" : "workspace-manager-apply",
                    backIdentifier: "workspace-manager-back", cancelIdentifier: "workspace-manager-close",
                    cancel: escape, back: { workspaces.pageList() }, primary: { workspaces.send(["type": "confirm"]) })
            } else {
            HStack(alignment: .top) {
                if subpage {
                    Button { workspaces.pageList() } label: { SharedIcon(name: "go-previous") }
                        .accessibilityLabel(common["back"].string).accessibilityIdentifier("workspace-manager-back")
                }
                Text(subpage ? view["details"]["title"].string : view["prompt"].isNull || workspaces.inlineRename ? view["title"].string : view["prompt"]["title"].string)
                    .font(.title2).fontWeight(.semibold)
                Spacer()
                if view["prompt"].isNull && toolbarMode {
                    Button(common["close"].string, role: .cancel) { escape() }
                        .keyboardShortcut(.cancelAction).disabled(workspaces.busy).accessibilityIdentifier("workspace-manager-close")
                }
            }
            }
            if !view["prompt"].isNull && !workspaces.inlineRename { WorkspaceManagerForm(workspaces: workspaces, spec: view["prompt"]) }
            else {
                if let error = message {
                    Text(error).foregroundStyle(.red).textSelection(.enabled).accessibilityIdentifier("workspace-manager-error")
                }
                if workspaces.page == "history" { history } else { browser }
            }
        }.padding(page ? 0 : 20).frame(maxWidth: page ? .infinity : nil, maxHeight: page ? .infinity : nil, alignment: .topLeading)
            .modifier(EditorDialogFrame(idealWidth: workspaces.page == "workspaces" ? 460 : workspaces.page == "history" ? 480 : 620, maxWidth: workspaces.page == "workspaces" ? 620 : 800,
            idealHeight: workspaces.page == "workspaces" ? workspaceRowsHeight+176 : 600, maxHeight: 900))
            .onGeometryChange(for: CGSize.self) { $0.size } action: { _ in workspaces.refreshLayout() }
            .onChange(of: workspaces.page) { _, _ in query = "" }
    }
    private func escape() {
        if workspaces.inlineRename { workspaces.cancelPrompt() }
        else {
            switch DialogUI.resolve(["kind": "escape", "subpage": subpage,
                "search_focused": searchFocused, "search_has_text": !query.isEmpty]).string {
            case "clear_search": query = ""
            case "back": workspaces.pageList()
            default: workspaces.dismiss()
            }
        }
    }
    private func rememberWorkspaceRowHeights(_ heights: [String: CGFloat]) {
        var measured = workspaceRowHeights
        measured.merge(heights) { _, height in height }
        if measured != workspaceRowHeights { workspaceRowHeights = measured }
    }
    private var browser: some View {
        VStack(alignment: .leading, spacing: 12) {
            if toolbarMode {
                HStack {
                    ForEach([("this_workspace", header["this_workspace"].string), ("toolbar_library", header["saved_toolbars"].string)], id: \.0) { page in
                        Button(page.1) { workspaces.open(page.0) }
                            .tint(workspaces.page == page.0 ? .accentColor : .secondary)
                    }
                }
            }
            if toolbarMode { HStack {
                TextField(header["search"].string, text: $query).editorSearchInput().textFieldStyle(.roundedBorder).focused($searchFocused)
                    .onChange(of: query) { _, value in workspaces.search(value) }.accessibilityIdentifier("workspace-manager-search")
                if workspaces.page == "this_workspace" {
                    Button(header["new_toolbar_action"].string) { workspaces.form(["type": "new_toolbar", "value": NSNull()]) }
                        .accessibilityIdentifier("workspace-action-new_toolbar")
                }
            } }
            if workspaces.page == "workspaces" {
                VStack(alignment: .leading,spacing:6) {
                    WorkspaceSwitcherRows(workspaces: workspaces, reportRowHeights: rememberWorkspaceRowHeights)
                        .frame(minHeight: 0, idealHeight: page ? nil : workspaceRowsHeight, maxHeight: page ? .infinity : workspaceRowsHeight)
                    HStack(spacing:4) {
                        Button {workspaces.form(["type":"new"])} label: {SharedIcon(name:"plus").frame(width:34,height:34)}
                            .buttonStyle(.plain).disabled(workspaces.busy && !workspaces.inlineRename).accessibilityLabel(header["new_workspace"].string).accessibilityIdentifier("workspace-action-new")
                        let action=view["rows"].array.first {$0["id"].string==view["selected"].string}?["options"].array.first {$0["action"]["type"].string=="delete"}
                        Button {if let action {workspaces.activate(action["action"])}} label: {SharedIcon(name:"delete").frame(width:34,height:34)}
                            .buttonStyle(.plain).disabled(workspaces.busy && !workspaces.inlineRename || action?["enabled"].bool != true).accessibilityLabel(common["delete"].string).accessibilityIdentifier("workspace-action-delete")
                    }
                }
            } else { HStack(alignment: .top, spacing: 16) {
                if !subpage { EditorScrollView {
                LazyVStack(spacing: 8) {
                    if view["rows"].array.isEmpty { Text(nativeCopy["header"]["no_items"].string).foregroundStyle(.secondary).padding(20) }
                    ForEach(view["rows"].array, id: \.managerID) { row in
                        selectableRow(row).padding(12).background(view["selected"].string == row["id"].string ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 10))
                            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-item-" + row["id"].string)
                    }
                }
                } }
                if toolbarMode && (!singlePane || subpage), !view["details"].isNull {
                    EditorScrollView {
                        VStack(alignment: .leading, spacing: 12) {
                            Text(view["details"]["title"].string).font(.headline)
                            Text(view["details"]["description"].string)
                            ForEach(view["details"]["actions"].array, id: \.managerActionID) { button in action(button) }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }.accessibilityIdentifier("workspace-manager-details")
                }
            } }
            if !toolbarMode && !page {
                HStack(spacing: 8) {
                    Spacer()
                    Button(common["cancel"].string, role: .cancel) { workspaces.dismiss() }.keyboardShortcut(.cancelAction)
                        .accessibilityIdentifier("workspace-manager-close")
                        .buttonStyle(WorkspaceManagerButtonStyle())
                    Button(view["primary"].string) { workspaces.send(["type": "confirm"]) }
                        .buttonStyle(WorkspaceManagerButtonStyle(primary: true))
                        .disabled(!view["enabled"].bool || workspaces.busy && !workspaces.inlineRename)
                        .accessibilityIdentifier("workspace-manager-apply")
                }
            }
            if workspaces.busy || view["loading"].bool { ProgressView().controlSize(.small) }
        }
    }
    private func selectableRow(_ row: JSON) -> some View {
        Button { if toolbarMode { workspaces.openDetails(row["id"].string) } else { workspaces.select(row["id"].string) } } label: { rowLabel(row).contentShape(Rectangle()) }
            .buttonStyle(.plain).accessibilityIdentifier("workspace-select-" + row["id"].string)
    }
    private func rowLabel(_ row: JSON) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(row["title"].string).fontWeight(.medium).fixedSize(horizontal: false, vertical: true)
            if !row["subtitle"].string.isEmpty { Text(row["subtitle"].string).font(.caption).foregroundStyle(.secondary) }
        }.frame(maxWidth: .infinity, alignment: .leading)
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
            if !page {
            HStack {
                Button(common["cancel"].string, role: .cancel) { workspaces.dismiss() }.keyboardShortcut(.cancelAction)
                    .buttonStyle(WorkspaceManagerButtonStyle())
                Button(view["primary"].string) { workspaces.send(["type": "confirm"]) }
                    .buttonStyle(WorkspaceManagerButtonStyle(primary: true))
                    .disabled(workspaces.busy || !view["enabled"].bool)
                    .accessibilityIdentifier("workspace-history-restore")
            }
            }
        }
    }
}

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
            .padding(.horizontal, 16).frame(minHeight: 34)
            .foregroundStyle(prominent ? palette.accentForeground : foreground)
            .background(prominent ? palette.accent : foreground.opacity(0.10))
            .overlay { if configuration.isPressed { Color.black.opacity(0.12) } }
            .clipShape(SquircleShape.control)
            .overlay { SquircleShape.control.strokeBorder(foreground.opacity(prominent ? 0 : 0.10)) }
            .opacity(enabled ? 1 : 0.55).contentShape(SquircleShape.control)
    }
}

private struct WorkspaceManagerForm: View {
    @Environment(\.capyInterfaceLanguage) private var language
    @Environment(\.capyCommonCopy) private var common
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.editorDialogPage) private var page
    @ObservedObject var workspaces: WorkspaceController
    let spec: JSON
    @FocusState private var nameFocused: Bool
    var body: some View {
        Group {
            if spec["dialog"]["role"].string == "alert", let store = workspaces.store {
                EditorAlertLayout(viewport: CGSize(width: store.snapshot["layout"]["viewport"][0].number,
                    height: max(0, store.snapshot["layout"]["viewport"][1].number - store.obscuredBottom)),
                    preferWide: spec["dialog"]["frame"]["prefer_wide"].bool,
                    fontPixels: CGFloat(store.catalog["text_size_pt"].number * 4 / 3)) {
                    Text(spec["title"].string).font(.headline).multilineTextAlignment(.center)
                    EditorScrollView { fields }
                    cancel
                    confirm
                }
            } else if page {
                VStack(spacing: 0) {
                    EditorPageHeader(intent: spec["dialog"], title: spec["title"].string, identifier: "workspace-form",
                        primaryLabel: spec["confirm"].string, primaryEnabled: !workspaces.busy && !workspaces.validatingName && workspaces.nameError == nil,
                        primaryIdentifier: "workspace-form-confirm", cancel: { workspaces.cancelPrompt() }, primary: { workspaces.submit() })
                    EditorScrollView { fields.padding(16) }
                }.modifier(EditorDialogFrame())
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    fields
                    Spacer(minLength: 16)
                    HStack { Spacer(); cancel; confirm }
                }
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("workspace-library-form")
            .onChange(of: workspaces.formName, initial: true) { _, _ in workspaces.validateName(required: !spec["name"].isNull) }
            .onChange(of: spec.stableKey) { _, _ in workspaces.validateName(required: !spec["name"].isNull) }
            .onChange(of: language) { _, _ in workspaces.validateName(required: !spec["name"].isNull) }
    }
    private var fields: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(spec["message"].string).fixedSize(horizontal: false, vertical: true)
            if !spec["name"].isNull {
                TextField(common["name"].string, text: $workspaces.formName).textFieldStyle(.roundedBorder)
                    .focused($nameFocused).onAppear { nameFocused = page }.accessibilityIdentifier("workspace-form-name")
            }
            if !spec["description"].isNull {
                TextField(nativeCopy["header"]["description"].string, text: $workspaces.formDescription, axis: .vertical)
                    .textFieldStyle(.roundedBorder).lineLimit(3...6).accessibilityIdentifier("workspace-form-description")
            }
            if !spec["choices"].array.isEmpty {
                Picker(spec["choice_label"].string, selection: $workspaces.formChoice) {
                    ForEach(spec["choices"].array, id: \.managerID) { choice in Text(choice["label"].string).tag(choice["id"].string) }
                }.accessibilityIdentifier("workspace-form-choice")
            }
            if let error = workspaces.nameError ?? workspaces.error { Text(error).foregroundStyle(.red).accessibilityIdentifier("workspace-form-error") }
        }
    }
    private var cancel: some View {
        Button(spec["cancel"].string.isEmpty ? common["cancel"].string : spec["cancel"].string, role: .cancel) { workspaces.cancelPrompt() }.keyboardShortcut(.cancelAction)
            .accessibilityIdentifier("workspace-form-cancel")
    }
    private var confirm: some View {
        Button(spec["confirm"].string, role: spec["destructive"].bool ? .destructive : nil) { workspaces.submit() }
            .keyboardShortcut(.defaultAction)
            .disabled(workspaces.busy || workspaces.validatingName || workspaces.nameError != nil)
            .accessibilityIdentifier("workspace-form-confirm")
    }
}

private extension JSON {
    var managerID: String { self["id"].string }
    var managerActionID: String { self["action"].stableKey }
}
