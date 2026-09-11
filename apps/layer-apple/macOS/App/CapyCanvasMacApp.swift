import SwiftUI

@main struct CapyCanvasMacApp: App {
    @NSApplicationDelegateAdaptor(PersistenceTermination.self) private var termination
    init() {
        // AppKit otherwise discards intermediate mouse/drag/tablet samples.
        NSEvent.isMouseCoalescingEnabled = false
    }
    var body: some Scene {
        WindowGroup(id: "editor") { EditorSessionScene { MacEditorScene(scene: $0) } }
            .windowStyle(.hiddenTitleBar)
            .defaultSize(width: 1200, height: 900)
            .commands { MacEditorCommands() }
    }
}

private struct MacEditorScene: View {
    // WindowGroup creates this state per window, never one renderer for the app.
    @StateObject private var store: EditorStore
    init(scene: String) { _store = StateObject(wrappedValue: EditorStore(platform: 1, scene: scene)) }
    var body: some View {
        EditorView(store: store, showsApplicationMenus: false) { MacMetalCanvas(store: store) }
            .frame(minWidth: 700, minHeight: 500)
            .focusedSceneValue(\.editorStore, store)
    }
}

@MainActor private final class PersistenceTermination: NSObject, NSApplicationDelegate {
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // AppKit must receive terminateLater before its eventual reply.
        DispatchQueue.main.async { EditorStore.confirmCloseAll { allowed in
            guard allowed else { sender.reply(toApplicationShouldTerminate: false); return }
            EditorStore.flushAll { saved in
            if saved { sender.reply(toApplicationShouldTerminate: true) }
            else {
                let alert = NSAlert()
                alert.messageText = "Some settings or workspace changes could not be saved."
                alert.informativeText = "You can return to the app or quit with the last saved settings."
                alert.addButton(withTitle: "Return to App"); alert.addButton(withTitle: "Quit Anyway")
                sender.reply(toApplicationShouldTerminate: alert.runModal() == .alertSecondButtonReturn)
            }
            }
        } }
        return .terminateLater
    }
}
