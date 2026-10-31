import SwiftUI

@MainActor protocol LayerInspectorOwner: ObservableObject {
    var propertiesOpen: Bool { get }
    var simple: Bool { get }
    var footerHeight: CGFloat { get }
    var disclosure: String? { get }
    var pagePosition: CGFloat { get }
    var sizingContext: String { get }
    var reduceMotion: Bool { get set }
    func panel(_ id: JSON) -> JSON
    func toggleProperties()
    func measureLayerPages(_ next: JSON)
}

extension ContentDrawerPresentation: LayerInspectorOwner {
    var propertiesOpen: Bool { model["layer_properties"].bool }
    var simple: Bool { false }
    var footerHeight: CGFloat { layerSizes.isNull ? 49 : layerSizes["footer_height"].number }
    var disclosure: String? { nil }
    func toggleProperties() { toggleLayerProperties() }
}

@MainActor final class BubbleLayerInspector: LayerInspectorOwner {
    @Published private(set) var propertiesOpen = false
    @Published private(set) var simple = true
    @Published private(set) var footerHeight: CGFloat = 49
    @Published private(set) var pagePosition: CGFloat = 0
    @Published private(set) var disclosure: String?
    @Published private var metricsReady = false
    private weak var store: EditorStore?
    private weak var item: BubblePresentation?
    private let drawing: String
    private var panels: [String: JSON] = [:]
    var reduceMotion = false
    var sizingContext: String { "\(simple):\(metricsReady)" }
    init(store: EditorStore, item: BubblePresentation) { self.store = store; self.item = item; drawing = store.drawingIdentity }
    private var current: Bool {
        guard let store, let item else { return false }
        return store.drawingIdentity == drawing && store.bubbles.items.contains { $0 === item } && !item.view["card"].isNull
    }
    func receive(_ card: JSON) {
        let ready = !card["metrics"].isNull
        if metricsReady != ready { metricsReady = ready }
        if disclosure != card["disclosure"].string { disclosure = card["disclosure"].string }
        let simple = !card["full"].bool
        if self.simple != simple { self.simple = simple }
        let open = card["properties"].bool
        if propertiesOpen != open {
            propertiesOpen = open
            withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { pagePosition = open ? 1 : 0 }
        }
    }
    func panel(_ id: JSON) -> JSON {
        if let panel = store?.panel(id.string), !panel.isNull { panels[id.string] = panel; return panel }
        return panels[id.string] ?? JSON()
    }
    func toggleProperties() { if current, let item { store?.bubbles.action(["type": "properties", "id": item.id]) } }
    func measureLayerPages(_ next: JSON) {
        guard current, let item, next["heights"].array.count == 2 else { return }
        if footerHeight != next["footer_height"].number { footerHeight = next["footer_height"].number }
        let accepted = item.view["card"]["metrics"]
        guard accepted.isNull || accepted[0].stableKey != next["measurement"].stableKey || accepted[1].number != next["properties_height"].number else { return }
        store?.bubbles.action(["type": "measure_card", "id": item.id, "measurement": next["measurement"].raw, "properties_height": next["properties_height"].raw])
    }
}
