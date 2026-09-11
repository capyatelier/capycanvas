import SwiftUI

@main struct CapyCanvasApp: App {
    var body: some Scene { WindowGroup(id: "editor") { EditorSessionScene { IPadEditorScene(scene: $0) } } }
}

private struct IPadEditorScene: View {
    @StateObject private var store: EditorStore
    @Environment(\.scenePhase) private var phase
    init(scene: String) { _store = StateObject(wrappedValue: EditorStore(platform: 0, scene: scene)) }
    var body: some View {
        // The editor uses shared absolute dock geometry. Let the keyboard cover
        // its lower region; shrinking/centering that fixed layout would move
        // the canvas and its top search fields offscreen.
        GeometryReader { geometry in
            EditorView(store: store) { MetalCanvas(store: store) }
                .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
        }.ignoresSafeArea()
            .statusBarHidden()
            .onChange(of: phase) { _, next in
                if next != .active {
                    store.input(["type": "blur"])
                    PersistenceBackground.flush(store)
                }
                else { store.wake?() }
            }
    }
}

@MainActor private enum PersistenceBackground {
    static func flush(_ store: EditorStore) {
        let lease = BackgroundLease()
        lease.identifier = UIApplication.shared.beginBackgroundTask(withName: "Save drawing recovery and preferences") {
            Task { @MainActor in lease.finish() }
        }
        store.flushPersistence { _ in lease.finish() }
    }
    @MainActor private final class BackgroundLease {
        var identifier = UIBackgroundTaskIdentifier.invalid
        func finish() {
            guard identifier != .invalid else { return }
            UIApplication.shared.endBackgroundTask(identifier); identifier = .invalid
        }
    }
}
