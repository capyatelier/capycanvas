import AppKit

/// Preserve SwiftUI's window delegate while adding the shared unsaved-document
/// decision. Other window callbacks continue to the original delegate.
final class DocumentWindowDelegate: NSObject, NSWindowDelegate {
    private weak var downstream: NSWindowDelegate?
    private weak var window: NSWindow?
    private weak var store: EditorStore?
    private var asking = false
    private var approved = false
    init(store: EditorStore) { self.store = store }
    @MainActor func attach(_ window: NSWindow?) {
        if let previous = self.window, previous.delegate === self { previous.delegate = downstream }
        self.window = window; downstream = window?.delegate
        window?.delegate = self
        store?.projectFiles.closeWindow = { [weak window] in window?.performClose(nil) }
    }
    @MainActor func windowShouldClose(_ sender: NSWindow) -> Bool {
        if approved || store?.state["document_file"]["close_ready"].bool == true {
            approved = false
            return downstream?.windowShouldClose?(sender) ?? true
        }
        guard !asking else { return false }
        guard let store else { return downstream?.windowShouldClose?(sender) ?? true }
        asking = true
        store.projectFiles.confirmClose { [weak self, weak sender] allowed in
            guard let self else { return }
            self.asking = false
            if allowed {
                self.approved = true
                // Avoid recursively closing while AppKit is still deciding the
                // original windowShouldClose call (an unmodified file is fast).
                DispatchQueue.main.async { sender?.performClose(nil) }
            }
        }
        return false
    }
    override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || (downstream?.responds(to: selector) ?? false)
    }
    override func forwardingTarget(for selector: Selector!) -> Any? {
        if downstream?.responds(to: selector) == true { return downstream }
        return super.forwardingTarget(for: selector)
    }
}
