import SwiftUI

struct WorkspaceSwitcherRows: View {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    @Environment(\.capyNativeCopy) private var nativeCopy
    // Match the web manager's 55-point content plus divider and Android's
    // 56dp row minimum. Keep the actual controls tall, not just their spacing.
    private let rowHeight: CGFloat = 56
    @ObservedObject var workspaces: WorkspaceController
    @StateObject private var interaction = WorkspaceRowInteraction()
    @State private var optionCaptions: [String: String] = [:]
    @State private var reorderCaptions: [String: String] = [:]
    @State private var switchCaptions: [String: String] = [:]
    @State private var optionTitles: [String: String] = [:]
    @FocusState private var focus: String?
    private var rows: [JSON] { workspaces.view["rows"].array }
    private var pinned: Set<String> { Set(workspaces.view["switcher"].array.map { $0["id"].string }) }
    private var available: Bool {
        workspaces.ready && !workspaces.readOnly && !workspaces.busy
            && !workspaces.switcherBusy && workspaces.view["prompt"].isNull
    }
    private func refreshCaptions() {
        var captions: [String: String] = [:], titles: [String: String] = [:]
        var reorder: [String: String] = [:], switches: [String: String] = [:]
        for row in rows {
            let id = row["id"].string, title = row["title"].string
            titles[id] = title
            reorder[id] = optionTitles[id] == title ? reorderCaptions[id] : NativeTextContext.caption(["type": "reorder", "title": title], language: interfaceLanguage)
            switches[id] = optionTitles[id] == title ? switchCaptions[id] : NativeTextContext.caption(["type": "switch_workspace", "title": title], language: interfaceLanguage)
            captions[id] = optionTitles[id] == title ? optionCaptions[id]
                : NativeTextContext.caption(["type": "options_for", "title": title], language: interfaceLanguage)
        }
        optionTitles = titles; optionCaptions = captions; reorderCaptions = reorder; switchCaptions = switches
    }
    var body: some View {
        EditorScrollView { content }
            .accessibilityIdentifier("workspace-manager-rows")
            .onAppear { refreshCaptions() }
            .onChange(of: interfaceLanguage) { _, _ in optionTitles = [:]; refreshCaptions() }
            .onChange(of: workspaces.view["rows"].stableKey) { _, _ in refreshCaptions() }
            .onScrollPhaseChange { _, phase in
                if phase != .idle && !interaction.contact.held && !interaction.contact.dragging { interaction.cancel() }
            }
    }
    private var content: some View {
        LazyVStack(spacing: 2) {
            if rows.isEmpty { Text(nativeCopy["header"]["no_items"].string).foregroundStyle(.secondary).padding(20) }
            ForEach(rows, id: \.workspaceRowID) { row in
                let id = row["id"].string
                HStack(spacing: 6) {
                    Button {} label: { SharedIcon(name: "grip", size: 12).frame(width: 16, height: rowHeight).contentShape(Rectangle()) }
                        .buttonStyle(.plain).foregroundStyle(.secondary)
                        .help(nativeCopy["header"]["drag_to_reorder"].string).accessibilityLabel(reorderCaptions[id] ?? "")
                        .accessibilityIdentifier("workspace-grip-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.grip))
                    Button {
                        if !interaction.contact.consumeClick() { workspaces.select(id) }
                    } label: {
                        Text(row["title"].string).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 8).frame(minHeight: rowHeight).contentShape(Rectangle())
                    }.buttonStyle(.plain).help(row["title"].string).accessibilityLabel(switchCaptions[id] ?? "")
                        .accessibilityIdentifier("workspace-select-" + id).focusable().focused($focus, equals: "row-" + id)
                        .accessibilityAddTraits(workspaces.view["selected"].string == id ? .isSelected : [])
                        .onKeyPress { key in
                            if key.key == KeyEquivalent("\u{F70D}") && key.modifiers.contains(.shift) {
                                interaction.showMenu(id); return .handled
                            }
                            return .ignored
                        }
                    if pinned.contains(id) {
                        SharedIcon(name: "pin").accessibilityHidden(false).accessibilityLabel(nativeCopy["header"]["show_top"].string)
                    }
                    if row["current"].bool {
                        SharedIcon(name: "check").accessibilityHidden(false).accessibilityLabel(nativeCopy["header"]["current_workspace"].string)
                    }
                    Button {
                        if interaction.menu == id { interaction.closeMenu() } else { interaction.showMenu(id) }
                    } label: { Text("⋮").frame(width: 34, height: rowHeight).contentShape(Rectangle()) }
                        .buttonStyle(.plain).accessibilityLabel(optionCaptions[id] ?? "")
                        .accessibilityIdentifier("workspace-options-" + id).focusable().focused($focus, equals: "options-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.options))
                }.padding(.horizontal, 6)
                    .background(workspaces.view["selected"].string == id ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 6))
                    .opacity(interaction.drag?.id == id ? 0.35 : 1)
                    .modifier(WorkspaceRowMeasurement(id: id))
                    .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-item-" + id)
                    .editorPopover(isPresented: Binding(get: { interaction.menu == id }, set: {
                        if !$0 && interaction.menu == id { interaction.closeMenu() }
                    }), placement: .inward) { menu(row) }
            }
        }.coordinateSpace(name: "workspace-manager-rows")
            .background(NativeReorderInput(model: interaction))
            .onPreferenceChange(WorkspaceRowFrames.self) { interaction.frames = $0 }
            .overlay(alignment: .topLeading) { overlays }
            .onAppear { update() }
            .onChange(of: workspaces.view["rows"].stableKey) { _, _ in update() }
            .onChange(of: available) { _, _ in update() }
            .onDisappear { interaction.cancel() }
            .onKeyPress(.escape) {
                guard interaction.menu != nil || interaction.contact.target != nil else { return .ignored }
                let id = interaction.menu ?? interaction.contact.target?.id
                interaction.cancel(); if let id { focus = "options-" + id }; return .handled
            }
    }
    private func update() {
        interaction.update(items: rows, enabled: available)
        interaction.commit = { [weak workspaces] id, before in
            workspaces?.send(["type": "edit_switcher", "edit": ["type": "move", "id": id, "before": before as Any? ?? NSNull()]])
        }
    }
    @ViewBuilder private var overlays: some View {
        if let hint = interaction.hint {
            Rectangle().fill(Color.accentColor).frame(height: 2).offset(y: hint.y - 1)
                .allowsHitTesting(false).accessibilityHidden(true)
        }
        if let drag = interaction.drag, let row = rows.first(where: { $0["id"].string == drag.id }) {
            HStack { SharedIcon(name: "grip", size: 12); Text(row["title"].string).lineLimit(1); Spacer() }
                .padding(.horizontal, 10).frame(width: drag.bounds.width, height: drag.bounds.height)
                .modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 6)))
                .shadow(radius: 4, y: 2)
                .offset(x: drag.bounds.minX, y: drag.bounds.minY + drag.point.y - drag.origin.y)
                .allowsHitTesting(false).accessibilityHidden(true)
        }
    }
    private func menu(_ row: JSON) -> some View {
        let id = row["id"].string, order = workspaces.view["order"].array.map(\.string)
        let index = order.firstIndex(of: id)
        let edit = { (value: [String: Any]) -> [String: Any] in ["type": "edit_switcher", "edit": value] }
        let preferences: [[String: Any]] = [
            ["label": nativeCopy["header"]["show_top"].string, "enabled": available, "selected": pinned.contains(id),
             "action": edit(["type": "show", "id": id, "visible": !pinned.contains(id)])],
            ["label": nativeCopy["header"]["move_up"].string, "enabled": available && (index ?? 0) > 0,
             "action": edit(["type": "move", "id": id, "before": index.flatMap { $0 > 0 ? order[$0 - 1] : nil } as Any? ?? NSNull()])],
            ["label": nativeCopy["header"]["move_down"].string, "enabled": available && index.map { $0 + 1 < order.count } == true,
             "action": edit(["type": "move", "id": id, "before": index.flatMap { $0 + 2 < order.count ? order[$0 + 2] : nil } as Any? ?? NSNull()])],
        ]
        let actions = row["actions"].array.filter { !$0["primary"].bool && $0["enabled"].bool }.map { item -> [String: Any] in
            ["label": item["label"].raw, "enabled": available, "action": item["action"].raw]
        }
        return EditorActionMenu(model: AppleContextMenu(JSON(["sections": [preferences, actions]])) { workspaces.activate($0) },
            width: 260, identifier: "workspace-row-menu") { interaction.closeMenu() }
    }
}

private extension JSON {
    var workspaceRowID: String { self["id"].string }
    var workspaceActionID: String { self["action"].stableKey }
}
