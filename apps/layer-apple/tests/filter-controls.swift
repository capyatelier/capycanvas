import AppKit
import SwiftUI

@MainActor private final class FilterControlGeometry { var frames: [String: CGRect] = [:] }

/// Real Filters panel and shared menus, using local native events and temporary
/// windows. No renderer, system menu automation or artist storage is involved.
@main final class FilterControlChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_FILTER_CAPTURES"]!)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let deadline = Date().addingTimeInterval(15)
            while store.state["filter_categories"].array.isEmpty {
                try require(Date() < deadline, "Filter catalog startup timed out")
                try await drain(0.01)
            }
            let categories = store.state["filter_categories"].array
            let layers = store.state["layers"].stableKey, document = store.state["document_file"].stableKey
            func action(_ value: [String: Any]) async throws {
                try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                    store.edit(value) { error in
                        if let error { done.resume(throwing: HostFailure(message: error)) }
                        else { done.resume() }
                    }
                }
            }
            for theme in ["light", "dark"] {
                try await action(["type": "set_theme", "theme": theme])
                let geometry = FilterControlGeometry(), palette = EditorPalette(source: store.state["palette"])
                let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 600, height: 480),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                defer { window.contentView = nil; window.close() }
                window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                let content = AdjustmentPanel(store: store).frame(width: 226, height: 400)
                    .padding(12).frame(width: 600, height: 480, alignment: .topLeading)
                    .coordinateSpace(name: "choice-capture").environment(\.measureChoices, true)
                    .environment(\.editorPopupStore, store)
                    .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                    .onPreferenceChange(ChoiceFrames.self) { geometry.frames = $0 }
                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
                    .foregroundStyle(palette["text"]).background(palette["panel"])
                    .modifier(EditorPopoverHost())
                let host = NSHostingView(rootView: content); window.contentView = host
                window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                try await drain(0.3)
                func capture(_ name: String) throws {
                    guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
                        throw HostFailure(message: "No filter panel bitmap")
                    }
                    window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                    try bitmap.representation(using: .png, properties: [:])!.write(to:
                        directory.appendingPathComponent("\(platform)-\(theme)-\(name).png"))
                }
                func click(_ point: CGPoint) async throws {
                    for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                        let event = NSEvent.mouseEvent(with: type, location: host.convert(point, to: nil),
                            modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                            windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 0)!
                        NSApp.postEvent(event, atStart: false)
                        try await drain()
                    }
                }
                func send(_ characters: String, _ code: UInt16) async throws {
                    try key(characters, code: code, window: window); try await drain()
                }
                guard let choice = geometry.frames["filter-category"] else {
                    throw HostFailure(message: "Filters must reuse the measured editor choice")
                }
                // 226-wide panel: 12 insets, 16 icon, two 6-point gaps, 48 search.
                try require(abs(choice.width - 138) < 0.5 && abs(choice.height - 34) < 0.5,
                    "Category allocation must match Web/Android: \(choice)")
                try capture("closed")
                for next in 1...categories.count {
                    try await click(CGPoint(x: choice.midX, y: choice.midY))
                    if next == 2 { try capture("selected-category-menu") }
                    try await send("\u{F701}", 125); try await send("\r", 36)
                    let expected = categories[next % categories.count]["id"]
                    try require(store.state["filter_picker"]["category"].stableKey == expected.stableKey,
                        "Down/Return must advance from the selected category")
                    try require(!store.state["adjustments"].array.isEmpty &&
                        store.state["adjustments"].array.allSatisfy { expected.isNull || $0["category"].string == expected.string },
                        "The native choice must publish only its category's filters")
                }
                let search = CGPoint(x: choice.maxX + 6 + 24, y: choice.midY)
                try await click(search)
                try require(!store.state["filter_picker"]["search"].isNull, "Search must open from its full-width button")
                for (text, code): (String, UInt16) in [("b", 11), ("l", 37), ("u", 32), ("r", 15)] {
                    try await send(text, code)
                }
                try require(store.state["filter_picker"]["search"].string == "blur", "Search must receive native text input")
                try capture("search")
                try await send("\u{1b}", 53)
                try require(store.state["filter_picker"]["search"].isNull, "Escape must close filter search as on Web")
                try require(store.state["layers"].stableKey == layers && store.state["document_file"].stableKey == document,
                    "Category and search navigation must preserve the document")
                try require(store.failure == nil, store.failure ?? "")
                print("PASS: platform \(platform), \(theme), all categories, selected keyboard navigation, search/Escape and unchanged document")
            }
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) }
            catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
