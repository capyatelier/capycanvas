import SwiftUI

struct EditorMenuButton<Label: View>: View {
    let menu: () -> AppleContextMenu
    var identifier = "editor-action-menu"
    @ViewBuilder let label: () -> Label
    @Environment(\.editorPopupStore) private var store
    @State private var active: AppleContextMenu?
    @State private var popupID = UUID()
    var body: some View {
        Button { active = active == nil ? menu() : nil } label: { label().contentShape(Rectangle()) }
            .accessibilityValue(active == nil ? "Collapsed" : "Expanded")
            .editorPopover(isPresented: Binding(get: { active != nil }, set: { if !$0 { active = nil } })) {
                EditorActionMenu(model: active ?? AppleContextMenu(JSON()) { _ in }, identifier: identifier) { active = nil }
            }
            .onChange(of: active != nil) { _, open in store?.workspace.popover(popupID, open: open) }
            .onDisappear { store?.workspace.popover(popupID, open: false) }
    }
}

/// Familiar vertical menu rows, checkmarks, separators and submenu navigation,
/// rendered by native controls on the editor's opaque palette.
struct EditorActionMenu: View {
    let model: AppleContextMenu
    var width: CGFloat? = 340
    var identifier = "editor-action-menu"
    var dismiss: () -> Void
    @State private var pages: [(item: AppleContextMenu.Item, index: Int)] = []
    @State private var focus: Int?
    private var sections: [[AppleContextMenu.Item]] { pages.last?.item.sections ?? model.sections }
    private var entries: [AppleContextMenu.Item] { sections.flatMap { $0 } }
    private var enabled: [Int] { entries.indices.filter { entries[$0].enabled } }
    private var initialFocus: Int? { enabled.first { entries[$0].selected == true } ?? enabled.first }
    var body: some View {
        ScrollViewReader { reader in
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    header
                    ForEach(sections.indices, id: \.self) { section in sectionView(section) }
                }.padding(6)
            }.onChange(of: focus) { _, index in if let index { reader.scrollTo(index) } }
        }.frame(minWidth: 0, idealWidth: width, maxWidth: width,
            minHeight: 0, idealHeight: menuHeight, maxHeight: menuHeight)
            .font(.system(size: 15))
            .accessibilityElement(children: .contain).accessibilityIdentifier(identifier)
            .onAppear { focus = initialFocus }
            .background(ShortcutKeyCapture(captured: key).frame(width: 1, height: 1))
    }
    private var menuHeight: CGFloat {
        let rows = entries.count * 36
        let separators = max(0, sections.count - 1) * 9
        let heading = pages.isEmpty && model.title.isEmpty ? 0 : 40
        return min(560, CGFloat(rows + separators + heading + 12))
    }
    @ViewBuilder private var header: some View {
        if let page = pages.last {
            Button { back() } label: {
                Label(page.item.label, systemImage: "chevron.left")
                    .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
            }.buttonStyle(.plain).padding(.horizontal, 10).accessibilityIdentifier("editor-menu-back")
            Divider()
        } else if !model.title.isEmpty {
            Text(model.title).foregroundStyle(.secondary).padding(10)
        }
    }
    @ViewBuilder private func sectionView(_ section: Int) -> some View {
        if section > 0 { Divider().padding(.vertical, 4) }
        ForEach(sections[section].indices, id: \.self) { row in
            itemView(sections[section][row], index: sections.prefix(section).reduce(0) { $0 + $1.count } + row)
        }
    }
    private func itemView(_ item: AppleContextMenu.Item, index: Int) -> some View {
        Button { activate(index) } label: { itemLabel(item) }
            .buttonStyle(.plain).disabled(!item.enabled)
            .accessibilityIdentifier(item.identifier)
            .accessibilityAddTraits(item.selected == true ? .isSelected : [])
            .id(index)
            .background(focus == index ? EditorPalette.sharedAccent.opacity(0.15) : .clear,
                in: RoundedRectangle(cornerRadius: 4))
            .onHover { if $0 && item.enabled { focus = index } }
            .keyboardShortcut(item.bindings.first.flatMap { menuShortcut($0) })
    }
    private func itemLabel(_ item: AppleContextMenu.Item) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "checkmark").frame(width: 16).opacity(item.selected == true ? 1 : 0)
            Text(item.label).frame(maxWidth: .infinity, alignment: .leading)
            if !item.hint.isEmpty { Text(item.hint).foregroundStyle(.secondary).font(.caption) }
            if !item.sections.isEmpty { Image(systemName: "chevron.right").font(.caption) }
        }.padding(.horizontal, 10).frame(minHeight: 36).contentShape(Rectangle())
    }
    private func key(_ key: String, command: Bool, shift: Bool, alt: Bool) {
        if let index = entries.firstIndex(where: { item in
            item.bindings.contains { binding in
                binding["key"].string == key.lowercased() && binding["command"].bool == command
                    && binding["shift"].bool == shift && binding["alt"].bool == alt
            }
        }) { activate(index); return }
        guard !command && !alt else { return }
        switch key {
        case "arrowdown": step(1)
        case "arrowup": step(-1)
        case "tab": step(shift ? -1 : 1)
        case "arrowright":
            if let focus, entries.indices.contains(focus), !entries[focus].sections.isEmpty { activate(focus) }
        case "arrowleft": if !pages.isEmpty { back() }
        case "escape": dismiss()
        case "enter", " ":
            if let focus, entries.indices.contains(focus) { activate(focus) }
        default: break
        }
    }
    private func step(_ delta: Int) {
        guard !enabled.isEmpty else { return }
        focus = enabled[((enabled.firstIndex(of: focus ?? -1) ?? 0) + delta + enabled.count) % enabled.count]
    }
    private func back() { focus = pages.removeLast().index }
    private func activate(_ index: Int) {
        let item = entries[index]
        guard item.enabled else { return }
        if !item.sections.isEmpty { pages.append((item, index)); focus = initialFocus }
        else { dismiss(); item.action?() }
    }
}
