import SwiftUI

/// Context models are queried only on activation. Never cache eligibility or
/// create a menu query for every tile during drawing/camera publication.
struct WorkspaceContext: ViewModifier {
    @ObservedObject var store: EditorStore
    let target: JSON
    @State private var generation = UUID()
    @State private var popupID = UUID()
    func body(content: Content) -> some View {
        content.nativeEditorContextMenu(identity: "\(store.state["document_file"]["epoch"].uint):\(target.stableKey)",
            load: loadNativeMenu, visibility: { store.workspace.popover(popupID, open: $0) })
            .onChange(of: target.stableKey) { _, _ in generation = UUID() }
            .onChange(of: store.state["document_file"]["epoch"].uint) { _, _ in generation = UUID() }
            .onDisappear { generation = UUID(); store.workspace.popover(popupID, open: false) }
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
