import SwiftUI

/// Retain a native contact independently of the SwiftUI source's lifetime.
/// WorkspacePresentation still submits all moves and drops through shared Rust.
@MainActor final class WorkspaceReorderInteraction: ObservableObject, NativeReorderModel {
    let contact = ReorderContact()
    var viewport = CGRect.zero
    @Published private(set) var menu = JSON()
    @Published var menuSize = CGSize(width: 360, height: 360)
    private var menuPoint = CGPoint.zero
    private var menuSource: String?
    private var menuTarget: String?
    private var request = UUID()
    private let popup = UUID()
    private weak var workspace: WorkspacePresentation?
    private weak var store: EditorStore?
    init(workspace: WorkspacePresentation, store: EditorStore?) {
        self.workspace = workspace; self.store = store
    }
    var enabled: Bool {
        guard let store else { return false }
        return store.failure == nil && !store.workspaceManager.presented
            && workspace?.hasPopover(excluding: popup) != true
            && ["picker", "toolbar_prompt", "toolbar_manager", "preferences"].allSatisfy { store.snapshot[$0].isNull }
            && store.state["customization"]["control"].isNull
    }
    var menuBounds: CGRect {
        let size = CGSize(width: min(360, viewport.width), height: min(menuSize.height, viewport.height))
        return CGRect(x: max(viewport.minX, min(menuPoint.x, viewport.maxX - size.width)),
            y: max(viewport.minY, min(menuPoint.y, viewport.maxY - size.height)), width: size.width, height: size.height)
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
        guard let workspace, let (id, source) = workspace.hitSource(at: point),
              let item = try? JSON.decode(source.item) else { return nil }
        return ReorderTarget(id: id, surface: source.surface,
            valid: { [weak self, weak workspace] dragging in
                guard let self, let workspace, self.enabled else { return false }
                if dragging { return workspace.contains(item) }
                guard let current = workspace.sourceInstances[id] else { return false }
                return current.item == source.item && current.surface == source.surface && !current.bounds.isEmpty
            },
            openContext: { [weak self] in self?.showMenu(source.context, source: id, at: point) },
            closeContext: { [weak self] in self?.closeMenu() },
            begin: { [weak workspace] in workspace?.start(item, point: $0) },
            move: { [weak workspace] in workspace?.move(item, point: $0) },
            finish: { [weak workspace] in workspace?.move(item, point: $0, released: true) },
            cancel: { [weak workspace] in workspace?.cancel(item) })
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
            menu = result; workspace?.popover(popup, open: !result.isNull)
        }
    }
    func closeMenu() {
        request = UUID(); menuSource = nil; menuTarget = nil
        if !menu.isNull { menu = JSON(); workspace?.popover(popup, open: false) }
    }
    func recognizeHold() { contact.recognizeHold(); workspace?.refreshChrome() }
    func cancel() { contact.cancel(); closeMenu(); workspace?.refreshChrome() }
}

struct WorkspaceContactMenu: View {
    @ObservedObject var interaction: WorkspaceReorderInteraction
    @ObservedObject var store: EditorStore
    @FocusState private var focused: Bool
    var body: some View {
        if !interaction.menu.isNull {
            ZStack(alignment: .topLeading) {
                Color.clear.contentShape(Rectangle()).onTapGesture { interaction.closeMenu() }
                    .accessibilityIdentifier("workspace-context-dismiss")
                WorkspaceMenu(store: store, menu: interaction.menu, width: interaction.menuBounds.width) { interaction.closeMenu() }
                    .frame(maxHeight: interaction.viewport.height)
                    .onGeometryChange(for: CGSize.self) { $0.size } action: { interaction.menuSize = $0 }
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
                    .shadow(radius: 6, y: 2)
                    .offset(x: interaction.menuBounds.minX, y: interaction.menuBounds.minY)
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .focusable().focused($focused).onAppear { focused = true }
                .onKeyPress(.escape) { interaction.cancel(); return .handled }
        }
    }
}
