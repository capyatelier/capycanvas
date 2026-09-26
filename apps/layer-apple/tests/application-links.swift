import AppKit
import SwiftUI

/// Run the actual editor's OpenURLAction callback without opening a browser,
/// attaching a Metal surface or touching the artist's settings and drawings.
@main struct ApplicationLinkChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 700),
                styleMask: [.borderless], backing: .buffered, defer: true)
            window.isReleasedWhenClosed = false
            defer { window.contentView = nil; window.close() }
            var accepted = false
            var urls: [URL] = []
            window.contentView = NSHostingView(rootView: EditorView(store: store) { Color.clear }
                .environment(\.openURL, OpenURLAction { url in
                    urls.append(url)
                    return accepted ? .handled : .discarded
                }))
            try await wait("Initial editor state") { !store.state.isNull && !store.catalog.isNull }
            let layers = store.state["layers"].stableKey
            for (index, command) in ["website", "source_code", "website"].enumerated() {
                accepted = index == 2
                store.invoke(command)
                try await wait("Link callback and request completion") {
                    urls.count == index + 1
                        && !store.state["requests"].array.contains { $0["kind"]["type"].string == "open_link" }
                }
                let expected = await withCheckedContinuation { continuation in
                    store.query(["type": "application_link", "link": command]) { continuation.resume(returning: $0.string) }
                }
                precondition(urls.last?.absoluteString == expected, "The native handler must receive the shared public URL")
                if accepted {
                    precondition(store.failure == nil && store.state["host_error"].isNull,
                        "An accepted browser handoff must clear its shared failure and remain silent")
                } else {
                    precondition(store.state["host_error"].string == "Could not open the link")
                    precondition(store.failure == "Could not open the link",
                        "A rejected browser handoff must surface the error, not silently finish")
                    store.failure = nil
                }
                precondition(store.state["layers"].stableKey == layers && !store.state["document_file"]["modified"].bool,
                    "Browser success or failure must preserve the drawing")
            }
            print("PASS platform \(platform): both rejected Help links surface an error, a later handoff succeeds, requests complete and artwork is unchanged")
        }
    }
}
