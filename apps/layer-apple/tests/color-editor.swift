import AppKit
import SwiftUI
import Vision

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
    @MainActor static func renderedTextBounds(_ view: NSView, labels: [String]) async throws -> [String: CGRect] {
        guard let window = view.window, let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            throw HostFailure(message: "Cannot capture the native color form")
        }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let image = bitmap.cgImage else { throw HostFailure(message: "Missing native color pixels") }
        let bounds = window.convertToScreen(view.convert(view.bounds, to: nil))
        return try await Task.detached {
            let request = VNRecognizeTextRequest()
            request.recognitionLevel = .accurate; request.usesLanguageCorrection = false
            try VNImageRequestHandler(cgImage: image).perform([request])
            let lines = (request.results ?? []).compactMap { $0.topCandidates(1).first }
            var result: [String: CGRect] = [:]
            for label in labels where !label.isEmpty {
                for line in lines {
                    guard let range = line.string.range(of: label) else { continue }
                    let remainder = line.string.replacingCharacters(in: range, with: "").trimmingCharacters(in: .whitespaces)
                    guard remainder.isEmpty || labels.contains(remainder), let box = try line.boundingBox(for: range)?.boundingBox else { continue }
                    result[label] = CGRect(x: bounds.minX + box.minX * bounds.width, y: bounds.minY + box.minY * bounds.height,
                        width: box.width * bounds.width, height: box.height * bounds.height)
                    break
                }
            }
            return result
        }.value
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
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil))
        try await wait("Color editor catalog", failure: { store.failure }) { !store.catalog["native_copy"].isNull }
        for (space, nativeAppearance) in ["Srgb", "DisplayP3", "AdobeRgb", "ProPhoto"].flatMap({ space in
            [NSAppearance.Name.aqua, .darkAqua].map { (space, $0) }
        }) {
            let original = JSON(["space": space, "rgba": [0.12345678, 0.23456789, 0.34567891, 213.0 / 65535]])
            for mode in ["unchanged", "rgb", "invalid"] {
                var received: JSON?
                let session = ColorEditorSession(controller: nil, store: nil, colors: JSON(NSNull()), value: original) { color, _ in received = color }
                if mode == "invalid" {
                    try require(!session.act(["op": "value", "row": 0, "index": 0, "text": "invalid"], target: "0-0"), "Only a valid value commits")
                    try require(session.blocked, "Invalid draft must block Use Color")
                    session.close(apply: true)
                    try require(received == nil, "Invalid draft must not publish")
                    print("PASS: \(space)/\(nativeAppearance.rawValue) rejected color draft cannot publish")
                    continue
                }
                let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 720, height: 520),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                window.appearance = NSAppearance(named: nativeAppearance)
                let host = NSHostingView(rootView: ColorEditor(session: session)
                    .environment(\.capyNativeCopy, store.catalog["native_copy"])
                    .background(Color(nsColor: window.backgroundColor)))
                host.sizingOptions.insert(.maxSize)
                window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                defer { window.contentView = nil; window.close() }
                try await Task.sleep(for: .milliseconds(100))
                host.updateConstraintsForSubtreeIfNeeded()
                let fitting = host.fittingSize
                try require([fitting.width, fitting.height].allSatisfy { $0.isFinite && $0 > 0 },
                    "Native color editor fitting size must be finite and nonempty")
                try require(fields(host).isEmpty, "Values show as text until they are tapped")
                if space == "Srgb" && mode == "unchanged" {
                    window.setContentSize(CGSize(width: 320, height: 700))
                    try await wait("Compact native color fields") {
                        host.layoutSubtreeIfNeeded()
                        let values = inputs(host).filter { ["0-0", "0-1", "0-2"].contains($0.identity) }
                        return values.count == 3 && values.allSatisfy { input in
                            let bounds = input.convert(input.bounds, to: host)
                            return bounds.minX >= 0 && bounds.maxX <= host.bounds.width
                        }
                    }
                    let labels = ["current", "new"].map { store.catalog["native_copy"]["color"][$0].string }
                    var captions: [String: CGRect] = [:]
                    try await wait("Compact captions reserve space before the hex readout", step: {
                        captions = try await renderedTextBounds(host, labels: labels)
                    }) {
                        host.layoutSubtreeIfNeeded()
                        guard let input = inputs(host).first(where: { $0.identity == "hex" }) else { return false }
                        let hex = window.convertToScreen(input.convert(input.bounds, to: nil))
                        return hex.width > 0 && hex.height > 0 && captions.count == 2 && captions.values.allSatisfy {
                            $0.width > 0 && $0.height > 0 && $0.minY >= hex.maxY
                        }
                    }
                    window.setContentSize(CGSize(width: 720, height: 520))
                }
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
                print("PASS: \(space)/\(nativeAppearance.rawValue) native color \(mode)")
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
