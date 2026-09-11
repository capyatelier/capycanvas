import SwiftUI

@main struct CapyCanvasMacApp: App {
    init() {
        // AppKit otherwise discards intermediate mouse/drag/tablet samples.
        NSEvent.isMouseCoalescingEnabled = false
    }
    var body: some Scene {
        WindowGroup { MacEditorScene() }
            .windowStyle(.hiddenTitleBar)
            .defaultSize(width: 1200, height: 900)
            .commands { MacEditorCommands() }
    }
}

private struct MacEditorScene: View {
    // WindowGroup creates this state per window, never one renderer for the app.
    @StateObject private var store = EditorStore(platform: 1)
    var body: some View {
        EditorView(store: store, showsApplicationMenus: false) { MacMetalCanvas(store: store) }
            .frame(minWidth: 700, minHeight: 500)
            .focusedSceneValue(\.editorStore, store)
    }
}
