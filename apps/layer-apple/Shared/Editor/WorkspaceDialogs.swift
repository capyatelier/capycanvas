import SwiftUI

struct WorkspaceDialogs: ViewModifier {
    @ObservedObject var store: EditorStore
    private var open: Bool {
        ["picker", "toolbar_prompt", "toolbar_manager"].contains { !store.snapshot[$0].isNull }
            || !store.state["customization"]["control"].isNull
    }
    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(get: { open }, set: { _ in })) {
            WorkspaceDialog(store: store).interactiveDismissDisabled()
        }
    }
}

private struct WorkspaceDialog: View {
    @ObservedObject var store: EditorStore
    private var snapshot: JSON { store.snapshot }
    var body: some View {
        Group {
            if !snapshot["toolbar_prompt"].isNull { prompt(snapshot["toolbar_prompt"]) }
            else if !snapshot["picker"].isNull { picker(snapshot["picker"]) }
            else if !snapshot["toolbar_manager"].isNull { manager(snapshot["toolbar_manager"]) }
            else { standalone }
        }.padding(24).frame(minWidth: 320, idealWidth: 500, maxWidth: 560)
            .id(!snapshot["toolbar_prompt"].isNull ? snapshot["toolbar_prompt"]["title"].string : "workspace")
    }
    private func action(_ name: String) { store.customize(["type": name]) }
    private func error(_ value: JSON) -> some View {
        Group { if !value["error"].isNull { Text(value["error"].string).foregroundStyle(.red).accessibilityIdentifier("workspace-validation") } }
    }
    private func picker(_ view: JSON) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(view["title"].string).font(.headline)
            if !view["name"].isNull {
                WorkspaceTextField(view["name_label"].string, value: view["name"].string) { store.customize(["type": "picker_name", "name": $0]) }
                    .accessibilityIdentifier("toolbar-name")
            }
            WorkspaceTextField(view["search_hint"].string, value: view["query"].string) { store.customize(["type": "picker_search", "query": $0]) }
                .accessibilityIdentifier("tool-picker-search")
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(view["choices"].array, id: \.workspaceChoiceKey) { choice in
                        Button {
                            store.customize(["type": "picker_select", "control": choice["control"].raw, "selected": !choice["selected"].bool])
                        } label: {
                            HStack(spacing: 12) {
                                SharedIcon(name: choice["icon"].string)
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(choice["label"].string).fontWeight(.medium)
                                    Text(choice["description"].string).font(.caption).foregroundStyle(.secondary)
                                }.frame(maxWidth: .infinity, alignment: .leading)
                                Image(systemName: choice["selected"].bool ? "checkmark.square.fill" : "square")
                            }.frame(minHeight: 50).padding(.horizontal, 8).contentShape(Rectangle())
                        }.buttonStyle(.plain).accessibilityIdentifier("tool-choice-" + choice["label"].string)
                            .accessibilityAddTraits(choice["selected"].bool ? .isSelected : [])
                        Divider()
                    }
                }
            }.frame(minHeight: 120, idealHeight: 340, maxHeight: 400)
            error(view)
            HStack {
                Text("\(view["selected_count"].uint) selected").foregroundStyle(.secondary)
                Spacer()
                Button("Cancel", role: .cancel) { action("cancel_tools") }.keyboardShortcut(.cancelAction)
                Button(view["confirm_label"].string) { action("confirm_tools") }
                    .keyboardShortcut(.defaultAction).disabled(!view["can_confirm"].bool)
                    .accessibilityIdentifier("tool-picker-confirm")
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("tool-picker")
    }
    private func prompt(_ view: JSON) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(view["title"].string).font(.headline)
            if !view["message"].string.isEmpty { Text(view["message"].string) }
            if !view["name"].isNull {
                WorkspaceTextField(view["name_label"].string, value: view["name"].string) { store.customize(["type": "toolbar_name", "name": $0]) }
                    .accessibilityIdentifier("toolbar-name")
            }
            error(view)
            HStack {
                Spacer()
                Button(view["cancel_label"].string, role: .cancel) { action("cancel_toolbar") }
                    .keyboardShortcut(.cancelAction).accessibilityIdentifier("toolbar-prompt-cancel")
                Button(view["confirm_label"].string, role: view["destructive"].bool ? .destructive : nil) { action("confirm_toolbar") }
                    .keyboardShortcut(.defaultAction).disabled(!view["can_confirm"].bool)
                    .accessibilityIdentifier("toolbar-prompt-confirm")
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("toolbar-prompt")
    }
    private func manager(_ view: JSON) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(view["title"].string).font(.headline)
            Text(view["description"].string).foregroundStyle(.secondary)
            ScrollView {
                VStack(spacing: 0) {
                    if view["toolbars"].array.isEmpty { Text(view["empty_label"].string).padding(40) }
                    ForEach(view["toolbars"].array, id: \.workspacePanelKey) { toolbar in
                        Button { store.customize(["type": "select_managed_toolbar", "panel": toolbar["panel"].raw]) } label: {
                            HStack(spacing: 12) {
                                SharedIcon(name: toolbar["icon"].string)
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(toolbar["title"].string)
                                    Text(toolbar["subtitle"].string).foregroundStyle(.secondary).font(.caption)
                                }.frame(maxWidth: .infinity, alignment: .leading)
                                SharedIcon(name: "check").opacity(toolbar["panel"].string == view["selected"].string ? 1 : 0)
                            }.padding(10).contentShape(Rectangle())
                        }.buttonStyle(.plain).accessibilityIdentifier("managed-toolbar-" + toolbar["panel"].string)
                        Divider()
                    }
                }
            }.frame(minHeight: 120, idealHeight: 250, maxHeight: 340)
            HStack {
                Button(view["close_label"].string, role: .cancel) { action("close_toolbar_manager") }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(view["delete_label"].string, role: .destructive) { store.customize(view["delete_action"].object) }
                    .disabled(view["delete_action"].isNull).accessibilityIdentifier("delete-managed-toolbar")
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("toolbar-manager")
    }
    private var standalone: some View {
        VStack(alignment: .leading, spacing: 14) {
            let control = store.state["customization"]["control"].string
            if control == "brush_color" { ColorPanel(store: store).frame(width: 320) }
            else if control == "brush_opacity" {
                NumberControl(store: store, label: "Brush opacity", value: store.state["brush"]["opacity"].number,
                    control: store.catalog["opacity"], identifier: "popup-brush-opacity") { value, completion in
                    store.edit(["type": "set_brush_opacity", "value": value], completion: completion)
                }
            }
            HStack { Spacer(); Button("Done", role: .cancel) { action("close_control") }.keyboardShortcut(.cancelAction) }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("toolbar-control-popup")
    }
}

private extension JSON {
    var workspaceChoiceKey: String { self["control"].stableKey }
    var workspacePanelKey: String { self["panel"].string }
}
