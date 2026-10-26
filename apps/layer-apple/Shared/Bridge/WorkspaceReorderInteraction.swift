import SwiftUI

/// Retain a native contact independently of the SwiftUI source's lifetime.
/// WorkspacePresentation still submits all moves and drops through shared Rust.
@MainActor final class WorkspaceReorderInteraction: ObservableObject, NativeReorderModel {
    let contact = ReorderContact()
    var viewport = CGRect.zero
    @Published private(set) var menu = JSON()
    private var menuPoint = CGPoint.zero
    private var menuSource: String?
    private var menuTarget: String?
    private var request = UUID()
    let popup = UUID()
    private weak var workspace: WorkspacePresentation?
    private weak var store: EditorStore?
    init(workspace: WorkspacePresentation, store: EditorStore?) {
        self.workspace = workspace; self.store = store
    }
    var edgeReach: EdgeReachCoordinator? { store?.edgeReach }
    var bubbleInput: BubbleInteraction? { store?.bubbles.input }
    func relinquishToBubble() { contact.transfer(); closeMenu(); workspace?.refreshChrome() }
    var enabled: Bool {
        guard let store else { return false }
        return store.failure == nil && store.workspaces?.presented != true
            && workspace?.hasPopover(excluding: popup) != true
            && ["picker", "toolbar_prompt", "toolbar_manager", "preferences"].allSatisfy { store.snapshot[$0].isNull }
            && store.state["customization"]["control"].isNull
    }
    var menuAnchor: CGRect {
        return CGRect(origin: menuPoint, size: CGSize(width: 1, height: 1))
    }
    func validate() {
        if contact.target != nil { _ = contact.validate() }
        if let menuSource, !enabled || workspace?.sourceInstances[menuSource]?.context != menuTarget { closeMenu() }
    }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard enabled, viewport.contains(point) else { closeMenu(); return nil }
        // A new contact belongs to the menu or its dismissal backdrop. In
        // particular, a Pencil dismissal must never fall through into paint.
        if !menu.isNull { return nil }
        closeMenu()
        guard let workspace, let (id, source) = workspace.hitSource(at: point), source.surface != .control,
              let item = try? JSON.decode(source.item) else { return nil }
        let select = tabSelection(item: item, at: point, workspace: workspace, store: store)
        return ReorderTarget(id: id, surface: source.surface, canDrag: source.canDrag,
            valid: { [weak self, weak workspace] dragging in
                guard let self, let workspace, self.enabled else { return false }
                if dragging { return workspace.contains(item) }
                guard let current = workspace.sourceInstances[id] else { return false }
                return current.item == source.item && current.surface == source.surface && !current.bounds.isEmpty
            },
            openContext: { [weak self] in self?.showMenu(source.context, source: id, at: point) },
            closeContext: { [weak self] in self?.closeMenu() },
            press: select,
            begin: { [weak workspace] in workspace?.start(item, point: $0) },
            move: { [weak workspace] in workspace?.move(item, point: $0) },
            finish: { [weak workspace] in workspace?.move(item, point: $0, released: true) },
            cancel: { [weak workspace] in workspace?.cancel(item) })
    }
    func edgeReachSource(at point: CGPoint, device: ReorderDevice) -> EdgeReachSource? {
        guard enabled, viewport.contains(point), menu.isNull,
              let workspace, let (_, source) = workspace.hitSource(at: point) else { return nil }
        let item = try? JSON.decode(source.item)
        let contact: EdgeReachContact
        switch source.surface {
        case .handle: contact = .pickup
        case .tile:
            switch item?["kind"].string {
            case "panel": contact = source.canDrag ? .pickup : .free
            case "column": contact = .free
            default: contact = .input
            }
        case .headerEditor: contact = .pickup
        case .control: contact = source.controlContainer ? .free : .input
        case .row, .swatch: contact = .input
        }
        let select = item.flatMap { tabSelection(item: $0, at: point, workspace: workspace, store: store) }
        return EdgeReachSource(contact: contact, select: select, containsControls: source.controlContainer)
    }
    private func tabSelection(item: JSON, at point: CGPoint, workspace: WorkspacePresentation?, store: EditorStore?) -> (() -> Void)? {
        guard item["kind"].string == "panel", let workspace, let store else { return nil }
        let groups = store.snapshot["layout"]["groups"].array
            + store.state["customization"]["column_drawers"].array.map { $0["tabs"].replacing("id", with: $0["tabs"]["group"]) }
        guard let tab = workspace.tabs.first(where: { tab in
            tab["bounds"].rect.contains(point) && groups.contains {
                $0["id"].uint == tab["group"].uint && $0["panels"][Int(tab["index"].uint)].string == item["panel"].string
            }
        }) else { return nil }
        let group = tab["group"], panel = item["panel"]
        return { [weak store] in
            store?.dispatch(["type": "select_panel_tab", "group": group.raw, "panel": panel.raw])
        }
    }
    func acceptsContext(at point: CGPoint) -> Bool {
        enabled && viewport.contains(point) && menu.isNull
            && workspace?.hitSource(at: point)?.1.context != nil
    }
    func context(at point: CGPoint) {
        guard acceptsContext(at: point), let (id, source) = workspace?.hitSource(at: point) else { return }
        showMenu(source.context, source: id, at: point)
    }
    func showMenu(_ target: String?, source: String, at point: CGPoint) {
        guard enabled, let target, let target = try? JSON.decode(target), let store else { return }
        let generation = UUID(); request = generation
        menuSource = source; menuTarget = target.stableKey; menuPoint = point
        store.query(["type": "context", "target": target.raw]) { [weak self] result in
            guard let self, request == generation, enabled,
                  workspace?.sourceInstances[source] != nil else { return }
            menu = result
        }
    }
    func closeMenu() {
        request = UUID(); menuSource = nil; menuTarget = nil
        if !menu.isNull { menu = JSON() }
    }
    func recognizeHold() { contact.recognizeHold(); workspace?.refreshChrome() }
    func cancelReorder() { contact.cancel(); closeMenu(); workspace?.refreshChrome() }
    func cancel() { cancelReorder(); bubbleInput?.cancel() }
}

struct WorkspaceContactMenu: View {
    @ObservedObject var interaction: WorkspaceReorderInteraction
    @ObservedObject var store: EditorStore
    var body: some View {
        let anchor = interaction.menuAnchor
        Color.clear.frame(width: anchor.width, height: anchor.height)
            .editorPopover(isPresented: Binding(get: { !interaction.menu.isNull }, set: {
                if !$0 { interaction.closeMenu() }
            }), placement: .below, owner: interaction.popup) {
                WorkspaceMenu(store: store, menu: interaction.menu) { interaction.closeMenu() }
            }
            .offset(x: anchor.minX, y: anchor.minY).allowsHitTesting(false)
    }
}
