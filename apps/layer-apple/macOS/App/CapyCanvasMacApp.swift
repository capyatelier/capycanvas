import SwiftUI

@main struct CapyCanvasMacApp: App {
    @NSApplicationDelegateAdaptor(PersistenceTermination.self) private var termination
    @FocusedValue(\.editorCommands) private var editor
    init() {
        // AppKit otherwise discards intermediate mouse/drag/tablet samples.
        NSEvent.isMouseCoalescingEnabled = false
        NSWindow.allowsAutomaticWindowTabbing = false
    }
    var body: some Scene {
        WindowGroup(id: "editor") { EditorSessionScene { MacEditorScene(scene: $0) } }
            .windowStyle(.hiddenTitleBar)
            .defaultSize(width: 1200, height: 900)
            .commands { MacEditorCommands(copy: { termination.applicationCopy }) }
            .onChange(of: editor, initial: true) { _, editor in
                guard let editor else { return }
                termination.setMenuTitles(editor.menuTitles, copy: editor.store.bootstrap)
            }
    }
}

private struct MacEditorScene: View {
    // WindowGroup creates this state per window, never one renderer for the app.
    @StateObject private var store: EditorStore
    init(scene: String) { _store = StateObject(wrappedValue: EditorStore(platform: 1, scene: scene, deferLaunch: true)) }
    var body: some View {
        EditorView(store: store) { MacMetalCanvas(store: store) }
            .frame(minWidth: 700, minHeight: 500)
            .navigationTitle(store.projectFiles.title)
            .focusedSceneValue(\.editorCommands, EditorCommandContext(store: store))
    }
}

@MainActor private final class PersistenceTermination: NSObject, NSApplicationDelegate {
    private var observers: [NSObjectProtocol] = []
    private let helpMenu = NSMenu()
    private var menuTitles: [String: String] = [:]
    private(set) var applicationCopy = JSON()
    private var menuUpdatePending = false
    func setMenuTitles(_ titles: [String: String], copy: JSON) {
        menuTitles = titles
        if !copy.isNull { applicationCopy = copy }
        updateMenusAfterConstruction()
    }
    private func updateMenusAfterConstruction() {
        guard !menuUpdatePending else { return }
        menuUpdatePending = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.menuUpdatePending = false
            let app = NSApplication.shared
            guard let root = app.mainMenu else { return }
            if app.helpMenu?.supermenu === root { self.removeHelpMenu() }
            projectMacMenuTitles(self.menuTitles, in: root, windowsMenu: app.windowsMenu)
        }
    }
    private func removeHelpMenu() {
        let app = NSApplication.shared
        guard app.helpMenu !== helpMenu || helpMenu.supermenu != nil else { return }
        if let menu = app.helpMenu, let parent = menu.supermenu {
            let index = parent.indexOfItem(withSubmenu: menu)
            if index >= 0 { parent.removeItem(at: index) }
        }
        app.helpMenu = helpMenu
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        removeHelpMenu()
        let center = NSWorkspace.shared.notificationCenter
        observers = [NSMenu.didAddItemNotification, NSMenu.didChangeItemNotification].map { name in
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] notification in
                guard let menu = notification.object as? NSMenu else { return }
                DispatchQueue.main.async { [weak self, weak menu] in
                    guard let menu, let root = NSApplication.shared.mainMenu,
                        menu === root || menu.supermenu === root else { return }
                    self?.updateMenusAfterConstruction()
                }
            }
        } + [
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
        DispatchQueue.main.async { EditorStore.flushAll { saved in
            if saved { EditorStore.detachWorkspaceOwners { sender.reply(toApplicationShouldTerminate: true) } }
            else {
                let alert = NSAlert()
                alert.messageText = self.applicationCopy["recovery"]["changes_not_saved"].string
                alert.informativeText = self.applicationCopy["recovery"]["quit_unsaved_description"].string
                alert.addButton(withTitle: self.applicationCopy["common"]["keep_open"].string); alert.addButton(withTitle: self.applicationCopy["recovery"]["quit_anyway"].string)
                let quit = alert.runModal() == .alertSecondButtonReturn
                if quit { EditorStore.detachWorkspaceOwners { sender.reply(toApplicationShouldTerminate: true) } }
                else { sender.reply(toApplicationShouldTerminate: false) }
            }
        } }
        return .terminateLater
    }
}
