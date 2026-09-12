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
    private var sleepObservers: [NSObjectProtocol] = []
    func applicationDidFinishLaunching(_ notification: Notification) {
        let center = NSWorkspace.shared.notificationCenter
        sleepObservers = [
            center.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { EditorStore.suspendWorkspaces() }
            },
            center.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { EditorStore.resumeWorkspaces() }
            }
        ]
    }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // AppKit must receive terminateLater before its eventual reply.
        DispatchQueue.main.async { EditorStore.confirmCloseAll { allowed in
            guard allowed else { sender.reply(toApplicationShouldTerminate: false); return }
            EditorStore.finishClosingAll { saved in
            if saved { sender.reply(toApplicationShouldTerminate: true) }
            else {
                let alert = NSAlert()
                alert.messageText = "Some changes could not be saved."
                alert.informativeText = "You can return to the app to retry or quit with the last saved copies."
                alert.addButton(withTitle: "Return to App"); alert.addButton(withTitle: "Quit Anyway")
                let quit = alert.runModal() == .alertSecondButtonReturn
                if !quit { EditorStore.resetCloseApprovals() }
                if quit { EditorStore.detachWorkspaceOwners { sender.reply(toApplicationShouldTerminate: true) } }
                else { sender.reply(toApplicationShouldTerminate: false) }
            }
            }
        } }
        return .terminateLater
    }
}
