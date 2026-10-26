import SwiftUI

struct WorkspaceSwitcherRows: View {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.editorPalette) private var palette
    private let rowHeight: CGFloat = 56
    @ObservedObject var workspaces: WorkspaceController
    var reportRowHeights: ([String: CGFloat]) -> Void = { _ in }
    @StateObject private var interaction = WorkspaceRowInteraction()
    @State private var reorderCaptions: [String: String] = [:]
    @State private var switchCaptions: [String: String] = [:]
    @State private var optionTitles: [String: String] = [:]
    @FocusState private var focus: String?
    @FocusState private var editing: Bool
    private var rows: [JSON] { workspaces.view["rows"].array }
    private var metrics: PanelControlMetrics { PanelControlMetrics(view: workspaces.store?.catalog["panel_metrics"] ?? JSON()) }
    private var available: Bool {
        workspaces.ready && !workspaces.readOnly && !workspaces.busy
            && !workspaces.switcherBusy && workspaces.view["prompt"].isNull
    }
    private func refreshCaptions() {
        var titles: [String: String] = [:]
        var reorder: [String: String] = [:], switches: [String: String] = [:]
        for row in rows {
            let id = row["id"].string, title = row["title"].string
            titles[id] = title
            reorder[id] = optionTitles[id] == title ? reorderCaptions[id] : NativeTextContext.caption(["type": "reorder", "title": title], language: interfaceLanguage)
            switches[id] = optionTitles[id] == title ? switchCaptions[id] : NativeTextContext.caption(["type": "switch_workspace", "title": title], language: interfaceLanguage)
        }
        optionTitles = titles; reorderCaptions = reorder; switchCaptions = switches
    }
    var body: some View {
        scroll.clipShape(SquircleShape.control)
            .accessibilityIdentifier("workspace-manager-rows")
            .onAppear { refreshCaptions() }
            .onChange(of: interfaceLanguage) { _, _ in optionTitles = [:]; refreshCaptions() }
            .onChange(of: workspaces.view["rows"].stableKey) { _, _ in refreshCaptions() }
    }
    @ViewBuilder private var scroll: some View {
        if #available(iOS 18.0, macOS 15.0, *) {
            EditorScrollView { content }.modifier(EditorScrollActivity(action: scrollStarted))
        } else {
            #if os(iOS)
            EditorScrollView { content.background(NativeScrollObservation(activity: scrollStarted)) }
            #endif
        }
    }
    private func scrollStarted() {
        if !interaction.contact.held && !interaction.contact.dragging { interaction.cancel() }
    }
    private var content: some View {
        LazyVStack(spacing: 0) {
            if rows.isEmpty { Text(nativeCopy["header"]["no_items"].string).foregroundStyle(.secondary).padding(20) }
            ForEach(rows, id: \.workspaceRowID) { row in
                let id = row["id"].string
                let renaming = workspaces.inlineRename && workspaces.view["prompt_action"]["value"].string == id
                HStack(spacing: 4) {
                    VStack(alignment:.leading,spacing:2) {
                        ZStack(alignment:.leading) {
                            Text(row["title"].string).lineLimit(1).padding(.horizontal,metrics.padding)
                                .frame(maxWidth:.infinity,minHeight:metrics.height,alignment:.leading)
                                .opacity(renaming ? 0 : 1).accessibilityHidden(renaming).allowsHitTesting(!renaming)
                                .contentShape(Rectangle()).gesture(TapGesture(count:2).onEnded {_ in rename(row)},
                                    including:row["options"].array.contains(where:{$0["action"]["type"].string=="rename" && $0["enabled"].bool}) ? .all : .none)
                                .accessibilityIdentifier("workspace-name-"+id)
                        if renaming {
                            TextField("",text:$workspaces.formName).textFieldStyle(.plain).focused($editing)
                                .padding(.horizontal,metrics.padding).frame(minHeight:metrics.height)
                                .background(palette.field,in:SquircleShape.control)
                                .accessibilityIdentifier("workspace-name-"+id)
                                .onSubmit {if !NativeTextContext.composing {workspaces.finishName()}}
                                .onAppear {editing=true;workspaces.validateName(required:true)}
                                .onChange(of:workspaces.formName) {_,_ in workspaces.validateName(required:true)}
                                .onChange(of:editing) {_,focused in if !focused {workspaces.finishName()}}
                                .onKeyPress(.escape) {guard !NativeTextContext.composing else{return .ignored};workspaces.cancelPrompt();return .handled}
                        }
                        }.frame(minHeight:metrics.height).accessibilityIdentifier("workspace-name-slot-"+id)
                        if !row["subtitle"].string.isEmpty {Text(row["subtitle"].string).font(.caption).foregroundStyle(.secondary).lineLimit(1).padding(.horizontal,metrics.padding)}
                    }.frame(maxWidth:.infinity,minHeight:rowHeight,alignment:.leading)
                        .contentShape(Rectangle()).onTapGesture {if !renaming && !interaction.contact.consumeClick(){workspaces.select(id)}}.focusable().focused($focus,equals:"row-"+id)
                        .modifier(WorkspaceRowAccessibility(id: id, label: switchCaptions[id] ?? "",
                            selected: workspaces.view["selected"].string == id, renaming: renaming,
                            select: { workspaces.select(id) }))
                        .onKeyPress {key in
                            guard !renaming else { return .ignored }
                            if key.key==KeyEquivalent("\u{F705}") {rename(row);return .handled}
                            if key.key==KeyEquivalent("\u{F70D}") && key.modifiers.contains(.shift){interaction.showMenu(id,reordering:false);return .handled}
                            if key.key == .return || key.key == .space {workspaces.select(id);return .handled}
                            return .ignored
                        }
                    if row["current"].bool {
                        SharedIcon(name: "check").accessibilityHidden(false).accessibilityLabel(nativeCopy["header"]["current_workspace"].string)
                    }
                    let pin = row["pin"]
                    Button {
                        workspaces.send(["type": "edit_switcher", "edit": pin["edit"].raw])
                    } label: {
                        SharedIcon(name: pin["icon"].string).frame(width: 34)
                            .frame(minHeight: rowHeight, maxHeight: .infinity).contentShape(Rectangle())
                    }.buttonStyle(.plain).foregroundStyle(pin["selected"].bool ? Color.accentColor : Color.secondary)
                        .disabled(!pin["enabled"].bool).help(pin["label"].string).accessibilityLabel(pin["label"].string)
                        .accessibilityAddTraits(pin["selected"].bool ? .isSelected : [])
                        .accessibilityIdentifier("workspace-pin-" + id).focusable().focused($focus, equals: "pin-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.pin))
                    Button {
                        if !interaction.contact.consumeClick() { interaction.showMenu(id) }
                    } label: { SharedIcon(name: "grip").frame(width: 16, height: 34).contentShape(Rectangle()) }
                        .buttonStyle(.plain).foregroundStyle(.secondary)
                        .help(nativeCopy["header"]["drag_to_reorder"].string).accessibilityLabel(reorderCaptions[id] ?? "")
                        .accessibilityIdentifier("workspace-grip-" + id).disabled(!available)
                        .focusable().focused($focus, equals: "grip-" + id)
                        .modifier(WorkspaceRowMeasurement(id: id, part: \.grip))
                }.padding(.leading,12-metrics.padding).padding(.trailing,12)
                    .background(workspaces.view["selected"].string == id ? Color.accentColor.opacity(0.10) : Color.primary.opacity(0.035))
                    .overlay(alignment:.top) {if id != rows.first?["id"].string {EditorLine(color: palette.settingsCardShade)}}
                    .offset(x:interaction.swipe==id ? -interaction.swipeOffset : 0)
                    .background(alignment:.trailing) {
                        if interaction.swipe==id,let action=row["options"].array.first(where:{$0["action"]["type"].string=="delete"}) {
                            Button(action["label"].string,role:.destructive){interaction.swipe=nil;interaction.swipeOffset=0;workspaces.activate(action["action"])}
                                .buttonStyle(.plain).foregroundStyle(.white).frame(width:72)
                                .frame(minHeight:rowHeight,maxHeight:.infinity).background(Color(red:0.78,green:0.16,blue:0.16)).accessibilityIdentifier("workspace-swipe-delete-"+id)
                        }
                    }
                    .clipped()
                    .opacity(interaction.drag?.id == id ? 0.35 : 1)
                    .modifier(WorkspaceRowMeasurement(id: id))
                    .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-item-" + id)
                    .editorPopover(isPresented: Binding(get: { interaction.menu == id }, set: {
                        if !$0 && interaction.menu == id { interaction.closeMenu() }
                    }), placement: .inward) { menu(row) }
            }
        }.coordinateSpace(name: "workspace-manager-rows")
            .background(NativeReorderInput(model: interaction))
            .onPreferenceChange(WorkspaceRowFrames.self) { frames in
                interaction.frames = frames
                reportRowHeights(frames.compactMapValues { $0.row.height > 0 ? $0.row.height : nil })
            }
            .overlay(alignment: .topLeading) { overlays }
            .onAppear { update() }
            .onChange(of: workspaces.view["rows"].stableKey) { _, _ in update() }
            .onChange(of: available) { _, _ in update() }
            .onDisappear { interaction.cancel() }
            .onKeyPress(.escape) {
                guard interaction.menu != nil || interaction.contact.target != nil else { return .ignored }
                let id = interaction.menu ?? interaction.contact.target?.id
                interaction.cancel(); if let id { focus = (interaction.reordering ? "grip-" : "row-") + id }; return .handled
            }
    }
    private func rename(_ row:JSON) {
        if let action=row["options"].array.first(where:{$0["action"]["type"].string=="rename" && $0["enabled"].bool}) {workspaces.activate(action["action"])}
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
            HStack { SharedIcon(name: "grip"); Text(row["title"].string).lineLimit(1); Spacer() }
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
            ["label": nativeCopy["header"]["move_up"].string, "enabled": available && (index ?? 0) > 0,
             "action": edit(["type": "move", "id": id, "before": index.flatMap { $0 > 0 ? order[$0 - 1] : nil } as Any? ?? NSNull()])],
            ["label": nativeCopy["header"]["move_down"].string, "enabled": available && index.map { $0 + 1 < order.count } == true,
             "action": edit(["type": "move", "id": id, "before": index.flatMap { $0 + 2 < order.count ? order[$0 + 2] : nil } as Any? ?? NSNull()])],
        ]
        let actions = row["options"].array.map { item -> [String: Any] in
            ["label": item["label"].raw, "enabled": available && item["enabled"].bool, "action": item["action"].raw]
        }
        return EditorActionMenu(model: AppleContextMenu(JSON(["sections": interaction.reordering ? [preferences] : [actions,preferences]])) { workspaces.activate($0) },
            width: 260, identifier: "workspace-row-menu") { interaction.closeMenu() }
    }
}

private extension JSON {
    var workspaceRowID: String { self["id"].string }
    var workspaceActionID: String { self["action"].stableKey }
}

private struct WorkspaceRowAccessibility: ViewModifier {
    let id: String
    let label: String
    let selected: Bool
    let renaming: Bool
    let select: () -> Void
    @ViewBuilder func body(content: Content) -> some View {
        let row = content.accessibilityElement(children: renaming ? .contain : .ignore)
            .accessibilityIdentifier("workspace-select-" + id)
            .accessibilityAddTraits(selected ? .isSelected : [])
        if renaming { row }
        else {
            row.accessibilityLabel(label).accessibilityAddTraits(.isButton)
                .accessibilityAction { select() }
        }
    }
}
