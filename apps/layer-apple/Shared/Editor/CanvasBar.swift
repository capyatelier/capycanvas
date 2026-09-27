import SwiftUI

@MainActor final class CanvasBarPresence: ObservableObject {
    enum Hold: Hashable { case contact, workspace }
    @Published private(set) var visible = true
    private var holds: Set<Hold> = []
    private var reappear: DispatchWorkItem?
    var delay: TimeInterval = 0.18
    func hold(_ source: Hold, _ held: Bool) {
        guard holds.contains(source) != held else { return }
        if held { holds.insert(source) } else { holds.remove(source) }
        reappear?.cancel(); reappear = nil
        if !holds.isEmpty { if visible { visible = false } } else if !visible { scheduleReturn() }
    }
    func interrupt() {
        reappear?.cancel(); reappear = nil
        if visible { visible = false }
        if holds.isEmpty { scheduleReturn() }
    }
    private func scheduleReturn() {
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.holds.isEmpty else { return }
            self.visible = true
        }
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
    @State private var reasons: [String: String] = [:]
    private static let itemHeight: CGFloat = 40, gap: CGFloat = 4, padding: CGFloat = 6, labelPadding: CGFloat = 6
    private static let preferences = JSON(["sliders": false, "text": true])
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]).glassy }
    private var textSize: CGFloat { store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3 }
    private var items: [JSON] { view["items"].array }
    private var completion: [JSON] { view["completion"].array }
    private var context: JSON { view["context"] }
    private func width(_ item: JSON) -> CGFloat {
        toolOptionSize(item["option"], vertical: false, width: 0, tile: CGSize(width: Self.itemHeight, height: Self.itemHeight),
            preferences: Self.preferences, textSize: textSize, caption: item["label"].string).width
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
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading).task(id: key) {
            store.query(["type": "canvas_bar_layout", "measure": measure]) { result in
                placed = result
                store.query(["type": "canvas_bar_menu", "context": context.raw, "shown": result["items"].uint]) { menu = $0 }
            }
            reasons = [:]
            for command in (items + completion).map({ $0["option"]["Action"]["state"] }) where !command.isNull && !command["enabled"].bool {
                store.query(["type": "canvas_bar_reason", "context": context.raw, "command": command["id"].raw]) { reply in
                    if !reply.isNull { reasons[command["id"].string] = reply.string }
                }
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
            EditorMenuButton(menu: { AppleContextMenu(menu) { store.dispatch($0) } }, identifier: "canvas-bar-menu",
                rootFocusesSelection: false) {
                SharedIcon(name: "more", size: 20).frame(width: Self.itemHeight, height: Self.itemHeight)
            }.buttonStyle(EditorControlButtonStyle(selected: false, corner: .half))
                .accessibilityLabel("More").help("More").accessibilityIdentifier("canvas-bar-more")
            ForEach(Array(completion.enumerated()), id: \.offset) { _, item in field(item, completion: true) }
        }.padding(Self.padding)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .foregroundStyle(palette["text"])
            .environment(\.editorPalette, palette)
            .glassSurface(SquircleShape.surface, fill: palette.glassPanel)
            .accessibilityElement(children: .contain).accessibilityIdentifier("canvas-action-bar")
            .background { OutsideShadow(shape: SquircleShape.surface, opacity: 0.16, radius: 4, y: 2).accessibilityHidden(true) }
    }
    private func field(_ item: JSON, completion: Bool) -> some View {
        let command = item["option"]["Action"]["state"]["id"].string
        return ToolOptionField(store: store, option: item["option"], iconSize: 20, vertical: false, labeled: false,
            style: "medium", preferences: Self.preferences, stacked: false, caption: item["label"].string, prefix: "canvas-bar",
            accent: completion && ["apply_transform", "complete_selection"].contains(command), reason: reasons[command], edit: edit)
            .frame(width: width(item), height: Self.itemHeight)
    }
}
