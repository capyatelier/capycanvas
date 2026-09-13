import SwiftUI

struct WorkspaceSwitcherRows: View {
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
        ScrollView { content }
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
                    Button {} label: { SharedIcon(name: "grip", size: 12).frame(width: 16, height: 32) }
                        .buttonStyle(.plain).foregroundStyle(.secondary)
                        .help("Drag to reorder").accessibilityLabel("Reorder " + row["title"].string)
                        .accessibilityIdentifier("workspace-grip-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.grip))
                    Button {
                        if !interaction.contact.consumeClick() { manager.select(id) }
                    } label: {
                        Text(row["title"].string).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 8).contentShape(Rectangle())
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
                    } label: { Text("⋮").frame(width: 26, height: 32) }
                        .buttonStyle(.plain).accessibilityLabel("Options for " + row["title"].string)
                        .accessibilityIdentifier("workspace-options-" + id).focusable().focused($focus, equals: "options-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.options))
                }.padding(.horizontal, 6)
                    .background(manager.selection == id ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 6))
                    .opacity(interaction.drag?.id == id ? 0.35 : 1)
                    .modifier(WorkspaceRowMeasurement(id: id))
                    .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-item-" + id)
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
        if let id = interaction.menu, let row = rows.first(where: { $0["id"].string == id }) {
            menu(row).frame(width: interaction.menuBounds.width, alignment: .leading)
                .onGeometryChange(for: CGSize.self) { $0.size } action: { interaction.menuSize = $0 }
                .modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 8)))
                .shadow(radius: 6, y: 2)
                .offset(x: interaction.menuBounds.minX, y: interaction.menuBounds.minY)
                .accessibilityElement(children: .contain).accessibilityLabel("Options for " + row["title"].string)
                .accessibilityIdentifier("workspace-row-menu")
        }
    }
    private func menu(_ row: JSON) -> some View {
        let preferences = row["switcher_actions"].array
        let secondary = row["actions"].array.filter { !$0["primary"].bool }
        let entries = preferences.map { ("preference-" + $0["id"].string, $0) }
            + secondary.map { ("action-" + $0["action"]["type"].string, $0) }
        let enabled = entries.filter { $0.1["enabled"].bool }.map(\.0)
        return VStack(alignment: .leading, spacing: 2) {
            ForEach(preferences, id: \.workspaceRowID) { item in
                menuAction(item, id: "preference-" + item["id"].string)
            }
            if !secondary.isEmpty { Divider() }
            ForEach(secondary, id: \.workspaceActionID) { item in
                menuAction(item, id: "action-" + item["action"]["type"].string)
            }
        }.padding(6)
            .onAppear { focus = enabled.first }
            .onKeyPress(.downArrow) { stepMenu(enabled, direction: 1); return .handled }
            .onKeyPress(.upArrow) { stepMenu(enabled, direction: -1); return .handled }
            .onKeyPress(.return) {
                guard available, let entry = entries.first(where: { $0.0 == focus }), entry.1["enabled"].bool else { return .ignored }
                interaction.closeMenu(); manager.activate(entry.1["action"]); return .handled
            }
    }
    private func stepMenu(_ ids: [String], direction: Int) {
        guard !ids.isEmpty else { return }
        let current = ids.firstIndex(of: focus ?? "") ?? 0
        focus = ids[(current + direction + ids.count) % ids.count]
    }
    private func menuAction(_ item: JSON, id: String) -> some View {
        Button {
            interaction.closeMenu(); manager.activate(item["action"])
        } label: {
            HStack(spacing: 8) {
                Group { if item["checked"].bool { Image(systemName: "checkmark") } else { Color.clear } }.frame(width: 16, height: 16)
                Text(item["label"].string); Spacer()
            }.padding(.horizontal, 6).frame(minHeight: 28).contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(!item["enabled"].bool || !available)
            .accessibilityValue(item["checked"].isNull ? "" : item["checked"].bool ? "On" : "Off")
            .accessibilityIdentifier("workspace-" + id).focusable().focused($focus, equals: id)
    }
}

private extension JSON {
    var workspaceRowID: String { self["id"].string }
    var workspaceActionID: String { self["action"].stableKey }
}
