import SwiftUI

struct WorkspaceSwitcherRows: View {
    // Match the web manager's 55-point content plus divider and Android's
    // 56dp row minimum. Keep the actual controls tall, not just their spacing.
    private let rowHeight: CGFloat = 56
    @ObservedObject var manager: WorkspaceManager
    @ObservedObject var library: WorkspaceLibrary
    @StateObject private var interaction = WorkspaceRowInteraction()
    @FocusState private var focus: String?
    private var rows: [JSON] { manager.view["rows"].array }
    private var available: Bool {
        library.ready && !library.readOnly && (!library.busy || library.previewingLayout)
            && !library.switcherBusy && !manager.processing && manager.prompt == nil
    }
    var body: some View {
        EditorScrollView { content }
            .accessibilityIdentifier("workspace-manager-rows")
            .onScrollPhaseChange { _, phase in
                if phase != .idle && !interaction.contact.held && !interaction.contact.dragging { interaction.cancel() }
            }
    }
    private var content: some View {
        LazyVStack(spacing: 2) {
            if rows.isEmpty { Text("No items found.").foregroundStyle(.secondary).padding(20) }
            ForEach(rows, id: \.workspaceRowID) { row in
                let id = row["id"].string
                HStack(spacing: 6) {
                    Button {} label: { SharedIcon(name: "grip", size: 12).frame(width: 16, height: rowHeight).contentShape(Rectangle()) }
                        .buttonStyle(.plain).foregroundStyle(.secondary)
                        .help("Drag to reorder").accessibilityLabel("Reorder " + row["title"].string)
                        .accessibilityIdentifier("workspace-grip-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.grip))
                    Button {
                        if !interaction.contact.consumeClick() { manager.select(id) }
                    } label: {
                        Text(row["title"].string).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 8).frame(minHeight: rowHeight).contentShape(Rectangle())
                    }.buttonStyle(.plain).help(row["title"].string)
                        .accessibilityIdentifier("workspace-select-" + id).focusable().focused($focus, equals: "row-" + id)
                        .accessibilityAddTraits(manager.selection == id ? .isSelected : [])
                        .onKeyPress { key in
                            if key.key == KeyEquivalent("\u{F70D}") && key.modifiers.contains(.shift) {
                                interaction.showMenu(id); return .handled
                            }
                            return .ignored
                        }
                    if row["pinned"].bool {
                        SharedIcon(name: "pin").accessibilityHidden(false).accessibilityLabel("Shown in top bar")
                    }
                    if library.status["active_id"].string == id {
                        SharedIcon(name: "check").accessibilityHidden(false).accessibilityLabel("Current workspace")
                    }
                    Button {
                        if interaction.menu == id { interaction.closeMenu() } else { interaction.showMenu(id) }
                    } label: { Text("⋮").frame(width: 34, height: rowHeight).contentShape(Rectangle()) }
                        .buttonStyle(.plain).accessibilityLabel("Options for " + row["title"].string)
                        .accessibilityIdentifier("workspace-options-" + id).focusable().focused($focus, equals: "options-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.options))
                }.padding(.horizontal, 6)
                    .background(manager.selection == id ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 6))
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
            .onChange(of: manager.view.stableKey) { _, _ in update() }
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
        interaction.commit = { [weak manager] id, before in
            manager?.activate(JSON(["type": "edit_switcher", "edit": ["type": "move", "id": id, "before": before as Any? ?? NSNull()]]))
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
        let sections = [row["switcher_actions"].array, row["actions"].array.filter { !$0["primary"].bool }]
            .map { $0.map { item in
                ["label": item["label"].raw, "enabled": item["enabled"].bool && available,
                 "selected": item["checked"].raw, "action": item["action"].raw]
            } }
        return EditorActionMenu(model: AppleContextMenu(JSON(["sections": sections])) { manager.activate($0) },
            width: 260, identifier: "workspace-row-menu") { interaction.closeMenu() }
    }
}

private extension JSON {
    var workspaceRowID: String { self["id"].string }
    var workspaceActionID: String { self["action"].stableKey }
}
