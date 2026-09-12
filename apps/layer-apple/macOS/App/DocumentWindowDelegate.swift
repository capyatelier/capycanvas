import AppKit

/// Preserve SwiftUI's window delegate while adding the shared unsaved-document
/// decision. Other window callbacks continue to the original delegate.
@MainActor final class DocumentWindowDelegate: NSObject, NSWindowDelegate {
    private weak var downstream: NSWindowDelegate?
    private weak var window: NSWindow?
    private weak var store: EditorStore?
    private var asking = false
    private var approved = false
    private var fullscreenTransition = false
    private var fullscreenTarget: Bool?
    private var fullscreenCompletion: WindowPresentation.Completion?
    init(store: EditorStore) { self.store = store }
    @MainActor func attach(_ window: NSWindow?) {
        if window != nil && self.window === window { return }
        store?.windowPresentation.changeFullscreen = nil
        store?.focusWindow = nil
        finishFullscreen(error: "The drawing window was detached")
        if let previous = self.window, previous.delegate === self { previous.delegate = downstream }
        self.window = window; downstream = window?.delegate
        window?.delegate = self
        store?.projectFiles.closeWindow = { [weak window] in window?.performClose(nil) }
        fullscreenTransition = false
        if let window {
            store?.focusWindow = { [weak window] in
                window?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            }
            store?.windowPresentation.observe(fullscreen: window.styleMask.contains(.fullScreen))
            store?.windowPresentation.changeFullscreen = { [weak self] target, completion in
                guard let self, self.window != nil else { completion(false, "The drawing window is unavailable"); return }
                self.fullscreenTarget = target; self.fullscreenCompletion = completion
                self.applyFullscreen()
            }
        }
    }

    private func applyFullscreen() {
        guard !fullscreenTransition, let window, let target = fullscreenTarget else { return }
        if window.styleMask.contains(.fullScreen) == target { finishFullscreen(); return }
        fullscreenTransition = true
        window.toggleFullScreen(nil)
    }
    private func finishFullscreen(error: String? = nil) {
        let completion = fullscreenCompletion
        fullscreenCompletion = nil; fullscreenTarget = nil
        completion?(window?.styleMask.contains(.fullScreen) == true, error)
    }
    private func observedFullscreen(error: String? = nil) {
        fullscreenTransition = false
        store?.windowPresentation.observe(fullscreen: window?.styleMask.contains(.fullScreen) == true)
        if let error { finishFullscreen(error: error) }
        else { applyFullscreen() }
    }
    func windowWillEnterFullScreen(_ notification: Notification) {
        fullscreenTransition = true
        downstream?.windowWillEnterFullScreen?(notification)
    }
    func windowWillExitFullScreen(_ notification: Notification) {
        fullscreenTransition = true
        downstream?.windowWillExitFullScreen?(notification)
    }
    func windowDidEnterFullScreen(_ notification: Notification) {
        downstream?.windowDidEnterFullScreen?(notification)
        observedFullscreen()
    }
    func windowDidExitFullScreen(_ notification: Notification) {
        downstream?.windowDidExitFullScreen?(notification)
        observedFullscreen()
    }
    func windowDidFailToEnterFullScreen(_ window: NSWindow) {
        downstream?.windowDidFailToEnterFullScreen?(window)
        observedFullscreen(error: "The drawing window could not enter full screen")
    }
    func windowDidFailToExitFullScreen(_ window: NSWindow) {
        downstream?.windowDidFailToExitFullScreen?(window)
        observedFullscreen(error: "The drawing window could not leave full screen")
    }
    @MainActor func windowShouldClose(_ sender: NSWindow) -> Bool {
        if approved {
            approved = false
            let allowed = downstream?.windowShouldClose?(sender) ?? true
            if !allowed { store?.cancelPreparedClose() }
            return allowed
        }
        guard !asking else { return false }
        guard let store else { return downstream?.windowShouldClose?(sender) ?? true }
        asking = true
        let confirm: (Bool) -> Void = { [weak self, weak sender] allowed in
            guard let self else { return }
            guard allowed else { self.asking = false; return }
            store.prepareClose { [weak self, weak sender] saved in
                guard let self else { return }
                self.asking = false
                if saved {
                    self.approved = true
                    DispatchQueue.main.async { sender?.performClose(nil) }
                } else {
                    store.cancelPreparedClose()
                    if store.workspaceLibrary?.error == nil && store.storageFailure == nil {
                        store.projectFiles.error = "Some changes could not be saved. Retry before closing this window."
                    }
                }
            }
        }
        if store.state["document_file"]["close_ready"].bool { confirm(true) }
        else { store.projectFiles.confirmClose(confirm) }
        return false
    }
    @MainActor func windowWillClose(_ notification: Notification) {
        store?.recovery.close()
        downstream?.windowWillClose?(notification)
    }
    func windowDidBecomeKey(_ notification: Notification) {
        downstream?.windowDidBecomeKey?(notification)
        if let library = store?.workspaceLibrary {
            Task { do { try await library.resume() } catch { library.error = error.localizedDescription } }
        }
    }
    override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || (downstream?.responds(to: selector) ?? false)
    }
    override func forwardingTarget(for selector: Selector!) -> Any? {
        if downstream?.responds(to: selector) == true { return downstream }
        return super.forwardingTarget(for: selector)
    }
}
