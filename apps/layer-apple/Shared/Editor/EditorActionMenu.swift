import SwiftUI

struct EditorMenuButton<Label: View>: View {
    let menu: (@escaping @MainActor (AppleContextMenu) -> Void) -> Void
    var identifier = "editor-action-menu"
    var rootFocusesSelection = true
    var isPresented: Binding<Bool>? = nil
    @ViewBuilder let label: () -> Label
    private var liveMenu: (() -> AppleContextMenu)?
    @State private var localActive = false
    @State private var model: AppleContextMenu?
    @State private var pending: UUID?
    private var active: Binding<Bool> { isPresented ?? $localActive }
    init(menu: @escaping () -> AppleContextMenu, identifier: String = "editor-action-menu",
        rootFocusesSelection: Bool = true, isPresented: Binding<Bool>? = nil, @ViewBuilder label: @escaping () -> Label) {
        self.init(menu: { $0(menu()) }, identifier: identifier, rootFocusesSelection: rootFocusesSelection,
            isPresented: isPresented, label: label)
        liveMenu = menu
    }
    init(menu: @escaping (@escaping @MainActor (AppleContextMenu) -> Void) -> Void, identifier: String = "editor-action-menu",
        rootFocusesSelection: Bool = true, isPresented: Binding<Bool>? = nil, @ViewBuilder label: @escaping () -> Label) {
        self.menu = menu; self.identifier = identifier; self.rootFocusesSelection = rootFocusesSelection
        self.isPresented = isPresented; self.label = label
    }
    var body: some View {
        Button {
            if active.wrappedValue || pending != nil {
                pending = nil; active.wrappedValue = false
            } else if liveMenu != nil {
                active.wrappedValue = true
            } else {
                let request = UUID(); pending = request
                menu { result in
                    guard pending == request else { return }
                    pending = nil; model = result; active.wrappedValue = true
                }
            }
        } label: { label().contentShape(Rectangle()) }
            .accessibilityValue(active.wrappedValue ? "Expanded" : "Collapsed")
            .editorPopover(isPresented: active) {
                if active.wrappedValue, let model = liveMenu?() ?? model {
                    EditorActionMenu(model: model, identifier: identifier,
                        rootFocusesSelection: rootFocusesSelection) { active.wrappedValue = false }
                }
            }
            .onChange(of: active.wrappedValue) { _, shown in if !shown { pending = nil; model = nil } }
            .onDisappear { pending = nil; model = nil }
    }
}

/// Familiar vertical menu rows, checkmarks, separators and submenu navigation,
/// rendered by native controls on the editor's opaque palette.
struct EditorActionMenu: View {
    @Environment(\.editorPalette) private var palette
    let model: AppleContextMenu
    var width: CGFloat? = 340
    var identifier = "editor-action-menu"
    var rootFocusesSelection = true
    var capturesKeys = true
    var dismiss: () -> Void
    @State private var pages: [Int] = []
    @State private var focus: Int?
    private var page: AppleContextMenu.Item? {
        var sections = model.sections, parent: AppleContextMenu.Item?
        for index in pages {
            let entries = sections.flatMap { $0 }
            guard entries.indices.contains(index) else { return nil }
            parent = entries[index]; sections = entries[index].sections
        }
        return parent
    }
    private var sections: [[AppleContextMenu.Item]] { pages.isEmpty ? model.sections : page?.sections ?? [] }
    private var entries: [AppleContextMenu.Item] { sections.flatMap { $0 } }
    private var enabled: [Int] { entries.indices.filter { entries[$0].enabled } }
    private var initialFocus: Int? {
        (rootFocusesSelection || !pages.isEmpty ? enabled.first { entries[$0].selected == true } : nil) ?? enabled.first
    }
    var body: some View {
        ScrollViewReader { reader in
            EditorScrollView {
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
            .background { if capturesKeys { ShortcutKeyCapture(captured: key).frame(width: 1, height: 1) } }
    }
    private var menuHeight: CGFloat {
        let rows = entries.count * 36
        let separators = max(0, sections.count - 1) * 9
        let heading = pages.isEmpty && model.title.isEmpty ? 0 : 40
        return min(560, CGFloat(rows + separators + heading + 12))
    }
    @ViewBuilder private var header: some View {
        if !pages.isEmpty {
            Button { back() } label: {
                Label(page?.label ?? "", systemImage: "chevron.left")
                    .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
            }.buttonStyle(.plain).padding(.horizontal, 10).accessibilityIdentifier("editor-menu-back")
            EditorLine(color: palette.panelBorder)
        } else if !model.title.isEmpty {
            Text(model.title).foregroundStyle(.secondary).padding(10)
        }
    }
    @ViewBuilder private func sectionView(_ section: Int) -> some View {
        if section > 0 { EditorLine(color: palette.panelBorder).padding(.vertical, 4) }
        ForEach(sections[section].indices, id: \.self) { row in
            itemView(sections[section][row], index: sections.prefix(section).reduce(0) { $0 + $1.count } + row)
        }
    }
    @ViewBuilder private func itemView(_ item: AppleContextMenu.Item, index: Int) -> some View {
        if item.isHeading {
            Text(item.label).foregroundStyle(.secondary).padding(.horizontal, 10).frame(minHeight: 36)
                .accessibilityAddTraits(.isHeader).id(index)
        } else {
        Button { activate(index) } label: { itemLabel(item) }
            .buttonStyle(.plain).disabled(!item.enabled)
            .accessibilityIdentifier(item.identifier)
            .accessibilityAddTraits(item.selected == true ? .isSelected : [])
            .id(index)
            .background(focus == index ? palette.accent.opacity(0.15) : .clear,
                in: RoundedRectangle(cornerRadius: 4))
            .onHover { if $0 && item.enabled { focus = index } }
            .keyboardShortcut(item.bindings.first.flatMap { menuShortcut($0) })
        }
    }
    private func itemLabel(_ item: AppleContextMenu.Item) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "checkmark").frame(width: 16).opacity(item.selected == true ? 1 : 0)
            if !item.icon.isEmpty { SharedIcon(name: item.icon) }
            Text(item.label).frame(maxWidth: .infinity, alignment: .leading)
            if item.marker { ToolGroupMarker() }
            if !item.hint.isEmpty { Text(item.hint).foregroundStyle(.secondary).font(.caption) }
            if !item.sections.isEmpty { Image(systemName: "chevron.right").font(.caption) }
        }.padding(.horizontal, 10).frame(minHeight: 36).contentShape(Rectangle())
    }
    private func key(_ key: String, command: Bool, shift: Bool, alt: Bool) {
        if let item = model.actions.first(where: { item in
            item.enabled && item.bindings.contains { binding in
                binding["key"].string == key.lowercased() && (binding["command"].bool || binding["control"].bool) == command
                    && binding["shift"].bool == shift && binding["alt"].bool == alt
            }
        }) { if !item.keepOpen { dismiss() }; item.action?(); return }
        guard !command && !alt else { return }
        switch key {
        case "arrowdown": step(1)
        case "arrowup": step(-1)
        case "home": focus = enabled.first
        case "end": focus = enabled.last
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
    private func back() { focus = pages.removeLast() }
    private func activate(_ index: Int) {
        let item = entries[index]
        guard item.enabled else { return }
        if !item.sections.isEmpty { pages.append(index); focus = initialFocus }
        else { if !item.keepOpen { dismiss() }; item.action?() }
    }
}
