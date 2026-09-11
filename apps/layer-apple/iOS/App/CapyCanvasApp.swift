import SwiftUI

@main struct CapyCanvasApp: App {
    @StateObject private var store = EditorStore(platform: 0)
    @Environment(\.scenePhase) private var phase
    var body: some Scene {
        WindowGroup {
            EditorView(store: store) { MetalCanvas(store: store) }
                .statusBarHidden()
                .onChange(of: phase) { _, next in
                    if next != .active { store.input(["type": "blur"]) }
                    else { store.wake?() }
                }
        }
    }
}
