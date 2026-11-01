import SwiftUI

@MainActor final class CanvasBarPresence: ObservableObject {
    @Published private(set) var visible = false
    @Published private(set) var bounds: CGRect?
    @Published private(set) var layout = JSON()
    private var context = JSON()
    private var held: UInt32?
    private var reappear: DispatchWorkItem?
    var delay: TimeInterval = 0
    var reveal: ((JSON, UInt32) -> Void)?
    private func show(_ value: Bool) {
        if visible != value { visible = value }
        let next = value && !layout.isNull ? layout["bounds"].rect : nil
        if bounds != next { bounds = next }
    }
    func receive(_ presentation: JSON) {
        guard !presentation.isNull else {
            reappear?.cancel(); reappear = nil
            context = JSON(); held = nil
            if !layout.isNull { layout = JSON() }
            show(false)
            return
        }
        let nextContext = presentation["context"], next = presentation["layout"]
        let changed = context.stableKey != nextContext.stableKey
        if changed {
            reappear?.cancel(); reappear = nil
            context = nextContext; held = nil
        }
        let retained = !next.isNull ? next : changed ? JSON() : layout
        if layout.stableKey != retained.stableKey { layout = retained }
        let hold = UInt32(truncatingIfNeeded: presentation["hold"].uint)
        if next.isNull {
            show(false)
            self.hold(hold)
        } else {
            held = hold
            reappear?.cancel(); reappear = nil
            show(true)
        }
    }
    func hold(_ value: UInt32) {
        guard value != held else { return }
        held = value
        reappear?.cancel(); reappear = nil
        show(false)
        guard value % 2 == 0 && !context.isNull else { return }
        let captured = context
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.reappear != nil, self.held == value, self.context.stableKey == captured.stableKey else { return }
            self.reappear = nil
            self.reveal?(captured, value)
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
        if !view.isNull {
            PlacedCanvasBar(store: store, view: view, placed: presence.layout, visible: presence.visible).id(view["context"].stableKey)
                .opacity(presence.visible ? 1 : 0)
                .allowsHitTesting(presence.visible)
                .accessibilityHidden(!presence.visible)
        }
    }
}

private struct PlacedCanvasBar: View {
    @ObservedObject var store: EditorStore
    let view: JSON
    let placed: JSON
    let visible: Bool
    private static let captionOpacity = 0.8, textGap: CGFloat = 6, chevron: CGFloat = 12
    private static let preferences = JSON(["sliders": false, "text": true])
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]).glassy }
    private var textSize: CGFloat { store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3 }
    private var metrics: BarMetrics { store.barMetrics }
    private var items: [JSON] { view["items"].array }
    private var completion: [JSON] { view["completion"].array }
    private var context: JSON { view["context"] }
    private func width(_ item: JSON) -> CGFloat {
        (item["separated"].bool ? BarMetrics.dividerSlot + metrics.gap : 0) + fieldWidth(item)
    }
    private func fieldWidth(_ item: JSON) -> CGFloat {
        if !item["menu"].isNull {
            if item["icon_only"].bool { return metrics.tile }
            return 2 * metrics.clear + 2 * BarMetrics.textPadding + metrics.icon + 2 * Self.textGap + Self.chevron
                + ceil(toolbarTextWidth(item["label"].string, size: textSize))
        }
        return toolOptionSize(item["option"], vertical: false, width: 0, tile: CGSize(width: metrics.tile, height: metrics.tile),
            preferences: Self.preferences, textSize: textSize, bar: metrics, caption: item["label"].string, language: store.interfaceLanguage).width
    }
    private var labelWidth: CGFloat {
        view["label"].isNull ? 0 : ceil(toolbarTextWidth(view["label"].string, size: textSize)) + BarMetrics.captionLeading + BarMetrics.captionTrailing
    }
    private var measure: [String: Any] {
        ["context": context.raw, "label": labelWidth, "items": items.map(width), "completion": completion.map(width),
         "more": metrics.tile, "height": metrics.tile, "gap": metrics.gap, "padding": 0]
    }
    private func edit(_ action: Any, _ completion: @escaping @MainActor (String?) -> Void) {
        store.edit(["type": "canvas_bar_edit", "context": context.raw, "action": action], completion: completion)
    }
    var body: some View {
        let key = JSON(measure).stableKey
        ZStack(alignment: .topLeading) {
            if !placed.isNull {
                bar(shown: min(max(0, Int(placed["items"].uint)), items.count)).placed(placed["bounds"])
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .task(id: key) { store.dispatch(["type": "measure_canvas_bar", "measure": measure]) }
    }
    private func bar(shown: Int) -> some View {
        let hasLabel = !view["label"].isNull
        return HStack(spacing: 0) {
            if hasLabel {
                Text(view["label"].string).lineLimit(1).foregroundStyle(palette["text"].opacity(Self.captionOpacity))
                    .padding(.leading, BarMetrics.captionLeading).padding(.trailing, BarMetrics.captionTrailing)
                    .frame(width: labelWidth, alignment: .leading).accessibilityIdentifier("canvas-bar-label")
                    .overlay { blank(labelWidth) }
            }
            ForEach(Array(items.prefix(shown).enumerated()), id: \.offset) { index, item in
                field(item, leading: index > 0 || hasLabel)
            }
            if shown > 0 || hasLabel { blank(metrics.gap) }
            EditorMenuButton(menu: { completion in
                store.query(["type": "canvas_bar_menu", "context": context.raw, "shown": shown]) { result in
                    completion(AppleContextMenu(result) { store.dispatch($0) })
                }
            }, identifier: "canvas-bar-menu",
                rootFocusesSelection: false) {
                SharedIcon(name: "more", size: metrics.icon).frame(width: metrics.tile, height: metrics.tile)
            }.buttonStyle(EditorControlButtonStyle(selected: false, corner: .half))
                .accessibilityLabel(store.bootstrap["common"]["more"].string).help(store.bootstrap["common"]["more"].string).accessibilityIdentifier("canvas-bar-more")
            ForEach(Array(completion.enumerated()), id: \.offset) { _, item in field(item, leading: true) }
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            .foregroundStyle(palette["text"])
            .environment(\.editorPalette, palette)
            .glassSurface(SquircleShape.surface, fill: palette.glassPanel, register: visible)
            .accessibilityElement(children: .contain).accessibilityIdentifier("canvas-action-bar")
            .background { OutsideShadow(shape: SquircleShape.surface, opacity: 0.16, radius: 4, y: 2).accessibilityHidden(true) }
    }
    private func field(_ item: JSON, leading: Bool) -> some View {
        let gap = leading ? metrics.gap : 0
        return HStack(spacing: 0) {
            if item["separated"].bool {
                blank(gap + BarMetrics.dividerSlot + metrics.gap, dividerInset: gap)
            } else if leading { blank(gap) }
            content(item).frame(width: fieldWidth(item), height: metrics.tile)
        }
    }
    @ViewBuilder private func blank(_ width: CGFloat, dividerInset: CGFloat? = nil) -> some View {
        if width > 0 {
            ControlInput(contact: { _ in }).frame(width: width, height: metrics.tile)
                .overlay(alignment: .leading) {
                    if let dividerInset {
                        ToolbarDivider(vertical: false, color: palette["text"].opacity(0.3))
                            .frame(width: BarMetrics.dividerSlot, height: metrics.tile)
                            .offset(x: dividerInset).allowsHitTesting(false)
                    }
                }.accessibilityHidden(true)
        }
    }
    @ViewBuilder private func content(_ item: JSON) -> some View {
        let command = item["option"]["Action"]["state"]
        if !item["menu"].isNull {
            let tile = item["icon_only"].bool
            menuField(item, command: command)
                .frame(height: tile ? metrics.tile : metrics.control).padding(.horizontal, tile ? 0 : metrics.clear)
        } else {
            ToolOptionField(store: store, option: item["option"], iconSize: metrics.icon, vertical: false, labeled: false,
                style: "small", preferences: Self.preferences, stacked: false, bar: metrics, caption: item["label"].string, prefix: "canvas-bar",
                accent: item["accent"].bool, edit: edit)
        }
    }
    private func menuField(_ item: JSON, command: JSON) -> some View {
        let id = item["menu"].string, label = item["label"].string, tile = item["icon_only"].bool
        let disabled = !command.isNull && !command["enabled"].bool, reason = command.disabledReason
        return EditorMenuButton(menu: { completion in
            store.query(["type": "canvas_bar_choice_menu", "context": context.raw, "id": id]) { result in
                completion(AppleContextMenu(result) { store.dispatch($0) })
            }
        },
            identifier: "canvas-bar-menu-items-" + id, rootFocusesSelection: false) {
            Group {
                if tile {
                    SharedIcon(name: item["icon"].string, size: metrics.icon).frame(maxWidth: .infinity, maxHeight: .infinity)
                        .overlay(alignment: .bottomTrailing) { ToolGroupMarker() }
                } else {
                    HStack(spacing: Self.textGap) {
                        SharedIcon(name: item["icon"].string, size: metrics.icon)
                        Text(label).lineLimit(1).fixedSize()
                        SharedIcon(name: "chevron-down", size: Self.chevron)
                    }.padding(.horizontal, BarMetrics.textPadding).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }.buttonStyle(EditorControlButtonStyle(selected: false, corner: .half)).disabled(disabled)
            .opacity(disabled ? 0.36 : 1).help(reason ?? label).accessibilityLabel(label).accessibilityHint(reason ?? "")
            .accessibilityIdentifier("canvas-bar-menu-" + id)
            .modifier(DisabledExplanation(reason: reason, identifier: "canvas-bar-reason-" + id))
    }
}
