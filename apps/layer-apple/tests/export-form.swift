import AppKit
import SwiftUI

/// Hidden format-specific drafts must not block a different output format.
/// Native typing and the default Choose File action exercise the production form.
@main final class ExportFormChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
        let native = store.native!, surface = attachSurface(store, CGSize(width: 128, height: 128))
        defer { native.detach(); withExtendedLifetime(surface) {} }
        func wait(_ label: String, _ ready: () -> Bool) async throws {
            try await CapyTest.wait(label, failure: { store.failure }, step: { await frame(native) }, ready)
        }
        try await wait("Metal startup") { store.snapshot["shaders_ready"].bool }
        var destinations: [String] = []
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) }, save: { _, type, done in
            destinations.append(type.identifier); done(nil)
        }, create: { _, done in done(JSON(["extent": [64, 48], "color": ["space": "DisplayP3", "depth": "U16"], "background": "White"])) }, exportOptions: { _ in }))
        try await store.apply(["type": "invoke", "command": "new_document"])
        try await wait("New drawing") { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
        let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 600, height: 720), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
        }
        // Return invokes SwiftUI's native default action, including when its
        // button is virtual rather than backed by an NSButton instance.
        func choose() async throws {
            try key("\r", code: 36, window: window); try await drain()
        }
        for draft in ["0", "not a number"] { for format in ["Png", "Tiff"] {
            try await store.apply(["type": "invoke", "command": "export_document"])
            try await wait("Export loaded") { store.projectFiles.exportEditor?.loaded == true }
            let editor = store.projectFiles.exportEditor!
            let host = NSHostingView(rootView: ExportForm(editor: editor)); window.contentView = host
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            try await drain(0.4)
            func changeFormat(_ format: String) async throws {
                editor.change("format", JSON(format)); try await wait("Format change") { !editor.busy }
                try require(editor.error == nil, editor.error ?? ""); try await drain(0.2)
            }
            try await changeFormat("Jpeg")
            guard let field = fields(host).first(where: { $0.isEditable && $0.placeholderString == "JPEG quality (1–100)" }) else {
                throw HostFailure(message: "Missing native JPEG quality field")
            }
            field.selectText(nil); try await drain()
            for character in draft { try key(String(character), code: 0, window: window); try await drain(0.01) }
            try require((window.firstResponder as? NSTextView)?.string == draft, "Typed quality must reach the real field")
            let before = destinations.count
            try await choose(); try await wait("JPEG validation") { editor.error != nil || destinations.count > before }
            try require(editor.error != nil && destinations.count == before, "Visible invalid JPEG quality must be rejected")
            try await changeFormat(format)
            try require(!fields(host).contains { $0.isEditable && $0.placeholderString == "JPEG quality (1–100)" }, "Quality field must be hidden")
            try await choose(); try await wait("Export choice") { editor.error != nil || destinations.count > before }
            try require(editor.error == nil && destinations.count == before + 1,
                "Hidden JPEG draft must not block \(format): \(editor.error ?? "missing destination")")
            try require(editor.recipe["jpeg_quality"].uint == 90, "Other formats retain a valid stored JPEG quality")
            try await wait("Destination cancellation") { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            try require(!store.state["document_file"]["modified"].bool, "Export cancellation must not edit the drawing")
            window.contentView = nil
        } }
        try await store.apply(["type": "invoke", "command": "export_document"])
        try await wait("JPEG export loaded") { store.projectFiles.exportEditor?.loaded == true }
        let editor = store.projectFiles.exportEditor!
        let host = NSHostingView(rootView: ExportForm(editor: editor)); window.contentView = host
        try await drain(0.2)
        editor.change("format", JSON("Jpeg")); try await wait("JPEG format") { !editor.busy }; try await drain(0.2)
        guard let field = fields(host).first(where: { $0.isEditable && $0.placeholderString == "JPEG quality (1–100)" }) else {
            throw HostFailure(message: "Missing native JPEG quality field")
        }
        field.selectText(nil); try await drain()
        for character in "73" { try key(String(character), code: 0, window: window); try await drain(0.01) }
        let before = destinations.count
        try await choose(); try await wait("Valid JPEG export") { editor.error != nil || destinations.count > before }
        try require(editor.error == nil && destinations.count == before + 1 && editor.recipe["jpeg_quality"].uint == 73,
            "JPEG must still use its visible quality draft")
        try await wait("JPEG destination cancellation") { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
        try require(!store.state["document_file"]["modified"].bool, "Valid JPEG export must preserve artwork")
        note("PASS: native invalid JPEG typing rejects JPEG; PNG/TIFF ignore hidden drafts, reach destination selection and preserve artwork")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) } catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
