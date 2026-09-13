import SwiftUI

/// Context models are queried only on activation. Never cache eligibility or
/// create a menu query for every tile during drawing/camera publication.
struct WorkspaceContext: ViewModifier {
    @ObservedObject var store: EditorStore
    let target: JSON
    var openOnTap = false
    var doubleClick: (() -> Void)?
    @State private var menu = JSON()
    @State private var generation = UUID()
    @State private var popupID = UUID()
    func body(content: Content) -> some View {
        Group {
            if openOnTap { tapMenu(content) }
            else {
                content.nativeEditorContextMenu(identity: "\(store.state["document_file"]["epoch"].uint):\(target.stableKey)",
                    load: loadNativeMenu, visibility: { store.workspace.popover(popupID, open: $0) })
            }
        }
        .onChange(of: target.stableKey) { _, _ in generation = UUID(); menu = JSON() }
        .onChange(of: store.state["document_file"]["epoch"].uint) { _, _ in generation = UUID(); menu = JSON() }
        .onDisappear { generation = UUID(); menu = JSON(); store.workspace.popover(popupID, open: false) }
    }
    private func loadNativeMenu(_ completion: @escaping @MainActor (AppleContextMenu?) -> Void) {
        let request = UUID(), epoch = store.state["document_file"]["epoch"].uint
        generation = request
        store.query(["type": "context", "target": target.raw]) { result in
            guard generation == request, store.state["document_file"]["epoch"].uint == epoch, !result.isNull else {
                completion(nil); return
            }
            completion(AppleContextMenu(result) { action in
                guard generation == request, store.state["document_file"]["epoch"].uint == epoch else { return }
                store.dispatch(action)
            })
        }
    }
    private func tapMenu(_ content: Content) -> some View {
        content.editorContextAction(open)
            .simultaneousGesture(TapGesture(count: 2).exclusively(before: TapGesture()).onEnded { value in
                switch value { case .first: doubleClick?(); case .second: open() }
            }, isEnabled: openOnTap)
            .editorPopover(isPresented: Binding(get: { !menu.isNull }, set: { if !$0 { menu = JSON() } })) {
                WorkspaceMenu(store: store, menu: menu) { menu = JSON() }
            }
            .onChange(of: menu.isNull) { _, empty in store.workspace.popover(popupID, open: !empty) }
    }
    private func open() {
        let request = UUID(); generation = request
        store.query(["type": "context", "target": target.raw]) { result in
            guard generation == request else { return }
            menu = result
        }
    }
}

struct WorkspaceMenu: View {
    @ObservedObject var store: EditorStore
    let menu: JSON
    var width: CGFloat? = 360
    var dismiss: () -> Void = {}
    var body: some View {
        EditorActionMenu(model: AppleContextMenu(menu) { store.dispatch($0) }, width: width,
            identifier: "workspace-context-menu", dismiss: dismiss)
    }
}

/// Local drafts keep text/caret stable while the serial owner publishes edits.
struct WorkspaceTextField: View {
    let label: String
    let value: String
    let edit: (String) -> Void
    @State private var text: String
    @State private var pending: String?
    init(_ label: String, value: String, edit: @escaping (String) -> Void) {
        self.label = label; self.value = value; self.edit = edit
        _text = State(initialValue: value)
    }
    var body: some View {
        TextField(label, text: Binding(get: { text }, set: { text = $0; pending = $0; edit($0) }))
            .textFieldStyle(.roundedBorder)
            .onChange(of: value) { _, next in
                if pending == nil || pending == next { text = next; pending = nil }
            }
    }
}
