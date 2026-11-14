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
    init(scene: String) { _store = StateObject(wrappedValue: EditorStore(platform: 0, scene: scene, deferLaunch: true)) }
    private var deferredEdges: Edge.Set {
        var edges: Edge.Set = store.state["workspace"]["zen_mode"].bool ? [.top, .bottom] : []
        let edge = store.snapshot["layout"]["phone"]["edge"].string
        if !edge.isEmpty { edges.formUnion(edge == "top" ? [.top, .bottom] : [.leading, .trailing]) }
        return edges
    }
    var body: some View {
        // Keep the drawable in full-window coordinates. UIKit's keyboard guide
        // supplies workspace clearance for controls through the shared layout.
        GeometryReader { geometry in
            EditorView(store: store) { MetalCanvas(store: store) }
                .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
        }.ignoresSafeArea()
            .statusBarHidden()
            .modifier(PhoneSystemOverlays())
            .defersSystemGestures(on: deferredEdges)
            .onChange(of: store.bootstrap["launch_mark"]["icon"].string) { previous, icon in
                if !previous.isEmpty && !icon.isEmpty { HomeScreenIcon.show(icon) }
            }
            .onChange(of: phase) { _, next in
                if next != .active {
                    store.input(["type": "blur"])
                    store.workspaces?.suspend()
                    PersistenceBackground.flush(store)
                }
                else {
                    store.native?.redraw()
                    store.workspaces?.resume()
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
