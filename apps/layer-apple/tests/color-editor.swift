import AppKit
import SwiftUI

@MainActor private final class EscapeReceiver: NSResponder {
    var count = 0
    override func cancelOperation(_ sender: Any?) { count += 1 }
}

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
    @MainActor static func inputs(_ view: NSView) -> [ControlInput.InputView] {
        (view as? ControlInput.InputView).map { [$0] } ?? view.subviews.flatMap(inputs)
    }
    @MainActor static func canceledScrub(_ host: NSView, session: ColorEditorSession, window: NSWindow) throws {
        guard let input = inputs(host).first(where: { $0.identity == "0-0" }) else { throw HostFailure(message: "Missing native color readout") }
        let before = session.view["value"].stableKey
        for (type, point) in [(NSEvent.EventType.leftMouseDown, CGPoint(x: 8, y: 16)), (.leftMouseDragged, CGPoint(x: 8, y: -8))] {
            guard let event = NSEvent.mouseEvent(with: type, location: input.convert(point, to: nil), modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                eventNumber: 1, clickCount: 1, pressure: 1) else { throw HostFailure(message: "Missing scrub event") }
            if type == .leftMouseDown { input.mouseDown(with: event) } else { input.mouseDragged(with: event) }
        }
        try require(session.view["value"].stableKey != before, "Native upward readout motion previews a changed color")
        input.cancelOperation(nil)
        try require(session.view["value"].stableKey == before, "Native cancellation restores the complete pre-scrub color")
    }
    @MainActor static func unownedEscape() throws {
        let input = ControlInput.InputView(), bridge = NSResponder(), receiver = EscapeReceiver()
        input.nextResponder = bridge; bridge.nextResponder = receiver
        input.cancelOperation(nil)
        try require(receiver.count == 1, "An idle native readout forwards Escape to its containing dialog")
        input.nextResponder = nil
        input.cancelOperation(nil)
        try require(receiver.count == 1, "An idle native readout without a containing dialog has no cancellation target")
    }
    @MainActor static func run() async throws {
        try unownedEscape()
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            DispatchQueue.global(qos: .userInitiated).async {
                let prepared = #"{"saved":"","preferred_languages":["en"]}"#.withCString { capy_apple_prepare_launch($0, nil) }
                if prepared != 0 {
                    _ = capy_apple_launch(9, prepared, nil, nil)
                    capy_apple_launch_free(prepared)
                }
                done.resume()
            }
        }
        for space in ["Srgb", "DisplayP3", "AdobeRgb", "ProPhoto"] {
            let original = JSON(["space": space, "rgba": [0.12345678, 0.23456789, 0.34567891, 213.0 / 65535]])
            for mode in ["unchanged", "rgb", "invalid"] {
                var received: JSON?
                let session = ColorEditorSession(controller: nil, store: nil, colors: JSON(NSNull()), value: original) { color, _ in received = color }
                if mode == "invalid" {
                    try require(!session.act(["op": "value", "row": 0, "index": 0, "text": "invalid"], target: "0-0"), "Only a valid value commits")
                    try require(session.blocked, "Invalid draft must block Use Color")
                    session.close(apply: true)
                    try require(received == nil, "Invalid draft must not publish")
                    print("PASS: \(space) rejected color draft cannot publish")
                    continue
                }
                let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 720, height: 520),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                let host = NSHostingView(rootView: ColorEditor(session: session))
                window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                defer { window.contentView = nil; window.close() }
                try await Task.sleep(for: .milliseconds(100))
                try require(fields(host).isEmpty, "Values show as text until they are tapped")
                try canceledScrub(host, session: session, window: window)
                if mode == "rgb" {
                    try require(session.act(["op": "value", "row": 0, "index": 0, "text": "32"], target: "0-0"), "Only a valid value commits")
                    try await Task.sleep(for: .milliseconds(100))
                }
                if let path = ProcessInfo.processInfo.environment["CAPY_COLOR_CAPTURE"], let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) {
                    host.cacheDisplay(in: host.bounds, to: bitmap)
                    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
                }
                try require(window.makeFirstResponder(nil), "Focus must leave the color fields")
                try key("\r", code: 36, window: window)
                try await wait("Native color publication") { received != nil }
                guard let received else { throw HostFailure(message: "Color was not published") }
                if mode == "rgb" { try require(abs(received["rgba"][0].number - 32.0 / 255) < 1e-6, "Typed RGB must publish") }
                else { for i in 0..<3 { try require(Float(received["rgba"][i].number) == Float(original["rgba"][i].number), "Untouched RGB precision must survive the native form") } }
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
