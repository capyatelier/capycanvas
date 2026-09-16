import AppKit
import SwiftUI

/// Real native text entry and the shared tagged-color transport, without GPU,
/// simulator startup or persistent artist data.
@main struct ColorEditorChecks {
    @MainActor static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    @MainActor static func key(_ characters: String, code: UInt16, window: NSWindow) throws {
        for type: NSEvent.EventType in [.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code) else { throw HostFailure(message: "Missing native key event") }
            NSApp.postEvent(event, atStart: false)
        }
    }
    @MainActor static func fields(_ view: NSView) -> [NSTextField] {
        (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
    }
    @MainActor static func run() async throws {
        for space in ["Srgb", "DisplayP3", "AdobeRgb", "ProPhoto"] {
            let original = JSON(["space": space, "rgba": [0.12345678, 0.23456789, 0.34567891, 213.0 / 65535]])
            for mode in ["unchanged", "alpha", "rgb", "invalid"] {
                var received: JSON?
                let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 420, height: 560),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                let host = NSHostingView(rootView: ColorEditor(value: original, documentSpace: "ProPhoto") { received = $0 })
                window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                defer { window.contentView = nil; window.close() }
                try await Task.sleep(for: .milliseconds(100))
                if ["alpha", "rgb", "invalid"].contains(mode) {
                    let index = mode == "alpha" ? 3 : 0
                    let entries = fields(host)
                    try require(entries.count == 4, "Document RGB must expose four native text fields")
                    let field = entries[index]
                    try require(window.makeFirstResponder(field), "Color entry must accept focus")
                    guard let editor = field.currentEditor() as? NSTextView else { throw HostFailure(message: "No native color text editor") }
                    editor.setSelectedRange(NSRange(location: 0, length: editor.string.utf16.count))
                    editor.insertText(mode == "alpha" ? "37" : mode == "invalid" ? "invalid" : "-0.125", replacementRange: NSRange(location: NSNotFound, length: 0))
                    try await Task.sleep(for: .milliseconds(100))
                }
                if let path = ProcessInfo.processInfo.environment["CAPY_COLOR_CAPTURE"], let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) {
                    host.cacheDisplay(in: host.bounds, to: bitmap)
                    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
                }
                // Exercise SwiftUI's native default action. SwiftUI
                // buttons need not be backed by NSButton instances.
                try key("\r", code: 36, window: window)
                try await Task.sleep(for: .milliseconds(100))
                if mode == "invalid" {
                    try require(received == nil, "Invalid draft must not publish")
                } else {
                    guard let received else { throw HostFailure(message: "Color was not published") }
                    try require(received["space"].string == (mode == "rgb" ? "ProPhoto" : space), "Publication must preserve or intentionally change the named RGB space")
                    if mode == "rgb" { try require(received["rgba"][0].number == -0.125, "Extended RGB input must not be clipped") }
                    else { for i in 0..<3 { try require(Float(received["rgba"][i].number) == Float(original["rgba"][i].number), "Untouched RGB precision must survive the native form") } }
                    try require(Float(received["rgba"][3].number) == Float(mode == "alpha" ? 0.37 : original["rgba"][3].number), "Alpha must preserve precision or use the explicit percent edit")
                }
                print("PASS: \(space) native color \(mode)")
            }
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) }
            catch { print("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
