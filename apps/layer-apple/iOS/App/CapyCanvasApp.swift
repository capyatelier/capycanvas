import SwiftUI

@main struct CapyCanvasApp: App {
    @UIApplicationDelegateAdaptor(WorkspaceSceneLifecycle.self) private var lifecycle
    var body: some Scene {
        WindowGroup(id: "editor") { EditorSessionScene { IPadEditorScene(scene: $0) } }
            .defaultSize(width: 1200, height: 900)
    }
}

@MainActor private final class WorkspaceSceneLifecycle: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, didDiscardSceneSessions sceneSessions: Set<UISceneSession>) {
        EditorStore.discardSceneSessions(Set(sceneSessions.map(\.persistentIdentifier)))
    }
}

private struct IPadEditorScene: View {
    @StateObject private var store: EditorStore
    @Environment(\.scenePhase) private var phase
    init(scene: String) { _store = StateObject(wrappedValue: EditorStore(platform: 0, scene: scene)) }
    var body: some View {
        // Keep the drawable in full-window coordinates. UIKit's keyboard guide
        // supplies workspace clearance for controls through the shared layout.
        GeometryReader { geometry in
            EditorView(store: store) { MetalCanvas(store: store) }
                .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
        }.ignoresSafeArea()
            .statusBarHidden()
            .onChange(of: phase) { _, next in
                if next != .active {
                    store.input(["type": "blur"])
                    store.workspaceLibrary?.suspend()
                    PersistenceBackground.flush(store)
                }
                else {
                    store.native?.redraw()
                    if let library = store.workspaceLibrary {
                        Task { do { try await library.resume() } catch { library.error = error.localizedDescription } }
                    }
                    store.wake?()
                }
            }
    }
}

@MainActor private enum PersistenceBackground {
    static func flush(_ store: EditorStore) {
        let lease = BackgroundLease()
        lease.identifier = UIApplication.shared.beginBackgroundTask(withName: "Save drawing recovery and preferences") {
            // UIKit invokes expiration synchronously on MainActor, before suspension.
            lease.finish()
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
