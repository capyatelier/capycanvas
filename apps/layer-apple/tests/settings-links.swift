import AppKit
import SwiftUI
import Vision

/// Exercise the production Settings links with native input and supplied browser
/// results. No browser, Metal surface or artist storage is needed.
@main final class SettingsLinkChecks: NativeWorkspaceInputFixture {
    /// SwiftUI's in-process accessibility tree omits virtual Form children.
    /// Locate the actual rendered link text inside this window instead.
    @MainActor static func text(in view: NSView, capture: String? = nil) throws -> [VNRecognizedText] {
        view.layoutSubtreeIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            throw HostFailure(message: "Cannot capture the Settings form")
        }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        if let capture, let path = ProcessInfo.processInfo.environment["CAPY_CAPTURE_DIRECTORY"],
            let png = bitmap.representation(using: .png, properties: [:]) {
            try png.write(to: URL(fileURLWithPath: path).appendingPathComponent(capture + ".png"))
        }
        guard let image = bitmap.cgImage else { throw HostFailure(message: "Missing Settings pixels") }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate; request.usesLanguageCorrection = false
        try VNImageRequestHandler(cgImage: image).perform([request])
        return (request.results ?? []).compactMap { $0.topCandidates(1).first }
    }
    @MainActor static func run() async throws {
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            try await wait("Initial Settings owner", failure: { store.failure }) { !store.state.isNull && !store.catalog.isNull }
            try await uncover(store)
            store.invoke("about")
            try await wait("About page", failure: { store.failure }) { store.snapshot["preferences"]["page"].string == "about" }
            let layers = store.state["layers"].stableKey
            let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 900, height: 700),
                styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            defer { window.contentView = nil; window.close() }
            var accepted = false
            var urls: [URL] = []
            let browser = OpenURLAction { url in
                urls.append(url)
                return accepted ? .handled : .discarded
            }
            func settings(page: Bool) -> some View {
                SettingsView(store: store).environment(\.openURL, browser).environment(\.editorDialogPage, page)
            }
            let host = NSHostingView(rootView: settings(page: false))
            window.contentView = host; window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            try await drain(0.5)
            for id in ["website", "source_code"] {
                let row = store.snapshot["preferences"]["pages"].array.flatMap { $0["groups"].array }
                    .flatMap { $0["rows"].array }.first { $0["id"].string == id }!
                let label = row["kind"]["label"].string
                let expected = await withCheckedContinuation { continuation in
                    store.query(["type": "application_link", "link": id]) { continuation.resume(returning: $0.string) }
                }
                for success in [false, true] {
                    accepted = success
                    let count = urls.count
                    let rendered = try text(in: host, capture: "\(platform)-\(id)-before-\(success)")
                    guard let link = rendered.first(where: { $0.string.contains(label) }),
                        let range = link.string.range(of: label), let box = try link.boundingBox(for: range) else {
                        throw HostFailure(message: "Missing rendered Settings link \(label): \(rendered.map(\.string))")
                    }
                    let point = CGPoint(x: host.bounds.minX + box.boundingBox.midX * host.bounds.width,
                        y: host.bounds.minY + (host.isFlipped ? 1 - box.boundingBox.midY : box.boundingBox.midY) * host.bounds.height)
                    try require(host.bounds.contains(point),
                        "Settings link must be visible in the owned window")
                    for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                        try event(type, at: point, marker: host, number: count + 1)
                        try await drain()
                    }
                    try await wait("Settings link callback", failure: { store.failure }) { urls.count == count + 1 }
                    try require(urls.last?.absoluteString == expected, "Settings must deliver the shared public URL")
                    try await drain(0.2)
                    let errorVisible = try text(in: host, capture: "\(platform)-\(id)-after-\(success)")
                        .contains { $0.string.contains("Could not open the link") }
                    try require(errorVisible == !success, "Settings must show rejected handoffs and clear their error after a successful retry")
                    try require(store.snapshot["preferences"]["page"].string == "about", "The About page must remain open")
                    try require(store.state["layers"].stableKey == layers && !store.state["document_file"]["modified"].bool,
                        "Link failure and retry must preserve the drawing")
                }
            }
            window.setContentSize(CGSize(width: 320, height: 568))
            host.rootView = settings(page: true)
            try await store.apply(["type": "preferences", "action": ["type": "page_list"]])
            try await drain()
            let rootText = try text(in: host)
            let appearancePage = store.snapshot["preferences"]["pages"].array.first { $0["id"].string == "appearance" }!
            let appearance = appearancePage["title"].string
            guard let line = rootText.first(where: { $0.string.contains(appearance) }),
                let range = line.string.range(of: appearance), let box = try line.boundingBox(for: range) else {
                throw HostFailure(message: "Narrow Settings root presents the shared page list")
            }
            let bounds = box.boundingBox
            let point = CGPoint(x: host.bounds.minX + host.bounds.width * 0.7,
                y: host.bounds.minY + (host.isFlipped ? 1 - bounds.midY : bounds.midY) * host.bounds.height)
            try require(point.x > host.bounds.minX + bounds.maxX * host.bounds.width + 8 && host.bounds.contains(point),
                "The Settings page contact must land in the row's blank area beyond its text")
            for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                try event(type, at: point, marker: host, number: 9); try await drain()
            }
            try await wait("The full Settings Appearance row activates its page", seconds: 5, failure: { store.failure }) {
                store.snapshot["preferences"]["page_requested"].bool && store.snapshot["preferences"]["page"].string == "appearance"
            }
            let theme = appearancePage["groups"].array.flatMap { $0["rows"].array }.first { $0["id"].string == "theme" }!
            try require(try text(in: host).contains { $0.string.contains(theme["title"].string) },
                "Activating Appearance must allocate and render its native preference Form")
            try await store.apply(["type": "preferences", "action": ["type": "search", "query": "github"]])
            let result = store.snapshot["preferences"]["search_results"].array.first { $0["title"].string == "Source code" }
            try require(result != nil && !store.snapshot["preferences"]["page_requested"].bool,
                "Settings search must publish the Source code result in its navigation pane")
            var results: [VNRecognizedText] = []
            try await wait("Narrow Settings search renders its published Source code result", seconds: 5,
                failure: { store.failure }, step: {
                    results = try text(in: host, capture: "\(platform)-search-results")
                }) { results.contains { $0.string.contains(result!["title"].string) } }
            guard let line = results.first(where: { $0.string.contains(result!["title"].string) }),
                let range = line.string.range(of: result!["title"].string), let box = try line.boundingBox(for: range) else {
                throw HostFailure(message: "Narrow Settings search must render its published Source code result")
            }
            let resultPoint = CGPoint(x: host.bounds.minX + box.boundingBox.midX * host.bounds.width,
                y: host.bounds.minY + (host.isFlipped ? 1 - box.boundingBox.midY : box.boundingBox.midY) * host.bounds.height)
            try require(host.bounds.contains(resultPoint), "The Settings search result must be visible in the owned window")
            for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                try event(type, at: resultPoint, marker: host, number: 10); try await drain()
            }
            try await wait("The rendered Settings search result opens About", seconds: 5, failure: { store.failure }) {
                let preferences = store.snapshot["preferences"]
                return preferences["page_requested"].bool && preferences["page"].string == "about" && preferences["query"].string.isEmpty
            }
            try require(try text(in: host).contains { $0.string.contains("github.com/capyatelier/capycanvas") },
                "Opening the Settings search result must render its shared Source code destination")
            note("PASS platform \(platform): native Settings Website/Source code rejection displays an error; accepted retry clears it and preserves artwork")
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) } catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
