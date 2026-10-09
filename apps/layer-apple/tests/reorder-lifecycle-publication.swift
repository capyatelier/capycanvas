import AppKit
import Combine

@main struct ReorderLifecyclePublicationChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 400, height: 300),
            styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        let model = DrawingTabInteraction(), input = ReorderInputView(frame: CGRect(x: 0, y: 0, width: 300, height: 200))
        input.model = model
        var publications = [String]()
        let subscription = model.objectWillChange.sink { publications.append(Thread.callStackSymbols.joined(separator: "\n")) }
        defer { withExtendedLifetime(subscription) {} }
        window.contentView!.addSubview(input)
        if let stack = publications.first { FileHandle.standardError.write(Data(("SYNCHRONOUS MOUNT PUBLICATION\n" + stack + "\n").utf8)) }
        try require(publications.isEmpty, "Initial native reorder mount must not synchronously publish SwiftUI model changes: \(publications.count)")
        func contact(_ id: String) {
            model.contact.prepare(ReorderTarget(id: id, surface: .row, valid: { _ in true },
                begin: { _ in }, move: { _ in }, finish: { _ in }, cancel: {}), device: .mouse, origin: .zero)
        }
        contact("initial")
        try await Task.sleep(for: .milliseconds(20))
        try require(model.contact.target?.id == "initial", "Deferred initial lifecycle work must preserve input begun after mounting")
        publications.removeAll()
        input.removeFromSuperview()
        try require(publications.isEmpty, "Native reorder unmount must not synchronously publish SwiftUI model changes")
        window.contentView!.addSubview(input)
        try require(publications.isEmpty, "Native reorder remount must not synchronously publish SwiftUI model changes")
        contact("remounted")
        try await Task.sleep(for: .milliseconds(20))
        try require(model.contact.target?.id == "remounted", "Old lifecycle cleanup must preserve a remounted contact")
        publications.removeAll()
        input.removeFromSuperview()
        try require(publications.isEmpty, "Final native unmount must not synchronously publish SwiftUI model changes")
        try await wait("Deferred native contact cancellation", seconds: 5) { model.contact.target == nil }
        print("PASS: actual native reorder mount/remount/unmount publishes outside the lifecycle pass and preserves newly begun contacts")
    }
}
