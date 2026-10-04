import SwiftUI

@MainActor final class CanvasBarPresence: ObservableObject {
    @Published private(set) var visible = true
    @Published var bounds: CGRect?
    private var held: UInt32 = 0
    private var reappear: DispatchWorkItem?
    var delay: TimeInterval = 0
    func hold(_ value: UInt32) {
        guard value != held else { return }
        held = value
        reappear?.cancel(); reappear = nil
        if visible { visible = false }
        guard value % 2 == 0 else { return }
        let work = DispatchWorkItem { [weak self] in self?.visible = true }
        reappear = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }
}

struct CanvasBarLayer: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var presence: CanvasBarPresence
    var body: some View {
        let view = store.state["canvas_bar"]
        if !view.isNull && presence.visible {
            PlacedCanvasBar(store: store, view: view).id(view["context"].stableKey)
        }
    }
}

private struct PlacedCanvasBar: View {
    @ObservedObject var store: EditorStore
    let view: JSON
    @State private var placed = JSON()
    @State private var menu = JSON()
    @State private var menus: [String: JSON] = [:]
    private static let itemHeight: CGFloat = 40, gap: CGFloat = 4, padding: CGFloat = 6, labelPadding: CGFloat = 6
    private static let menuPadding: CGFloat = 10, menuIcon: CGFloat = 20, menuGap: CGFloat = 6, menuChevron: CGFloat = 12
    private static let preferences = JSON(["sliders": false, "text": true])
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]).glassy }
    private var textSize: CGFloat { store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3 }
    private var items: [JSON] { view["items"].array }
    private var completion: [JSON] { view["completion"].array }
    private var context: JSON { view["context"] }
    private func width(_ item: JSON) -> CGFloat {
        if !item["menu"].isNull {
            return Self.menuPadding * 2 + Self.menuIcon + Self.menuGap * 2 + Self.menuChevron - 2
                + ceil(toolbarTextWidth(item["label"].string, size: textSize))
        }
        return toolOptionSize(item["option"], vertical: false, width: 0, tile: CGSize(width: Self.itemHeight, height: Self.itemHeight),
            preferences: Self.preferences, textSize: textSize, caption: item["label"].string, language: store.interfaceLanguage).width
    }
    private var labelWidth: CGFloat {
        view["label"].isNull ? 0 : ceil(toolbarTextWidth(view["label"].string, size: textSize)) + 2 * Self.labelPadding
    }
    private var measure: [String: Any] {
        ["context": context.raw, "label": labelWidth, "items": items.map(width), "completion": completion.map(width),
         "more": Self.itemHeight, "height": Self.itemHeight + 2 * Self.padding, "gap": Self.gap, "padding": Self.padding]
    }
    private func edit(_ action: Any, _ completion: @escaping @MainActor (String?) -> Void) {
        store.edit(["type": "canvas_bar_edit", "context": context.raw, "action": action], completion: completion)
    }
    var body: some View {
        let key = JSON(measure).stableKey + view.stableKey + store.snapshot["layout"]["work_area"].stableKey
        ZStack(alignment: .topLeading) {
            if !placed.isNull {
                bar(shown: min(max(0, Int(placed["items"].uint)), items.count)).placed(placed["bounds"])
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onChange(of: placed.stableKey, initial: true) { store.canvasBar.bounds = placed.isNull ? nil : placed["bounds"].rect }
            .onDisappear { store.canvasBar.bounds = nil }
            .task(id: key) {
            store.query(["type": "canvas_bar_layout", "measure": measure]) { result in
                placed = result
                store.query(["type": "canvas_bar_menu", "context": context.raw, "shown": result["items"].uint]) { menu = $0 }
            }
            for id in items.map({ $0["menu"] }).filter({ !$0.isNull }) {
                store.query(["type": "canvas_bar_choice_menu", "context": context.raw, "id": id.raw]) { menus[id.string] = $0 }
            }
        }
    }
    private func bar(shown: Int) -> some View {
        HStack(spacing: Self.gap) {
            if !view["label"].isNull {
                Text(view["label"].string).lineLimit(1).foregroundStyle(palette["text"].opacity(0.65))
                    .frame(width: labelWidth).accessibilityIdentifier("canvas-bar-label")
            }
            ForEach(Array(items.prefix(shown).enumerated()), id: \.offset) { _, item in field(item, completion: false) }
            EditorMenuButton(menu: { [menu] in AppleContextMenu(menu) { store.dispatch($0) } }, identifier: "canvas-bar-menu",
                rootFocusesSelection: false) {
                SharedIcon(name: "more", size: 20).frame(width: Self.itemHeight, height: Self.itemHeight)
            }.buttonStyle(EditorControlButtonStyle(selected: false, corner: .half))
                .accessibilityLabel(store.bootstrap["common"]["more"].string).help(store.bootstrap["common"]["more"].string).accessibilityIdentifier("canvas-bar-more")
            ForEach(Array(completion.enumerated()), id: \.offset) { _, item in field(item, completion: true) }
        }.padding(Self.padding)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .foregroundStyle(palette["text"])
            .environment(\.editorPalette, palette)
            .glassSurface(SquircleShape.surface, fill: palette.glassPanel)
            .accessibilityElement(children: .contain).accessibilityIdentifier("canvas-action-bar")
            .background { OutsideShadow(shape: SquircleShape.surface, opacity: 0.16, radius: 4, y: 2).accessibilityHidden(true) }
    }
    @ViewBuilder private func field(_ item: JSON, completion: Bool) -> some View {
        let command = item["option"]["Action"]["state"]
        if !item["menu"].isNull {
            menuField(item, command: command).frame(width: width(item), height: Self.itemHeight)
        } else {
            ToolOptionField(store: store, option: item["option"], iconSize: 20, vertical: false, labeled: false,
                style: "medium", preferences: Self.preferences, stacked: false, caption: item["label"].string, prefix: "canvas-bar",
                accent: item["accent"].bool, edit: edit)
                .frame(width: width(item), height: Self.itemHeight)
        }
    }
    private func menuField(_ item: JSON, command: JSON) -> some View {
        let id = item["menu"].string, label = item["label"].string
        let disabled = !command.isNull && !command["enabled"].bool, reason = command.disabledReason
        let tip = command.isNull ? label : reason ?? command["tooltip"].string
        return EditorMenuButton(menu: { [model = menus[id] ?? JSON()] in AppleContextMenu(model) { store.dispatch($0) } },
            identifier: "canvas-bar-menu-items-" + id, rootFocusesSelection: false) {
            HStack(spacing: Self.menuGap) {
                SharedIcon(name: item["icon"].string, size: Self.menuIcon)
                Text(label).lineLimit(1).fixedSize()
                SharedIcon(name: "chevron-down", size: Self.menuChevron).padding(.leading, -2)
            }.padding(.horizontal, Self.menuPadding).frame(maxWidth: .infinity, maxHeight: .infinity)
        }.buttonStyle(EditorControlButtonStyle(selected: false, corner: .half)).disabled(disabled)
            .opacity(disabled ? 0.36 : 1).help(tip).accessibilityLabel(label).accessibilityHint(reason ?? "")
            .accessibilityIdentifier("canvas-bar-menu-" + id)
            .modifier(DisabledExplanation(reason: reason, identifier: "canvas-bar-reason-" + id))
    }
}
