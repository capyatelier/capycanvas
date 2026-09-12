import AppKit
import SwiftUI

@MainActor private final class NumberGeometry { var frames: [String: CGRect] = [:] }

/// Real shared NumberControls and Rust formatting, on invisible AppKit surfaces.
/// This covers both Apple presets, not UIKit pixels or physical input delivery.
@main struct NumberControlCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        let env = ProcessInfo.processInfo.environment
        let directory = URL(fileURLWithPath: env["CAPY_NUMBER_CAPTURES"]!)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        precondition(NSImage(named: "icon-minus") != nil, "Set CAPY_TEST_ASSETS_APP")
        let widths: [CGFloat] = [160, 226, 320], width: CGFloat = 734, height: CGFloat = 652
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let deadline = Date().addingTimeInterval(15)
            while store.catalog.isNull || store.state.isNull {
                guard Date() < deadline else { throw HostFailure(message: "Number owner startup timed out") }
                try await Task.sleep(for: .milliseconds(5))
            }
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                store.edit(["type": "invoke", "command": "auto_select"]) { error in
                    if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                }
            }
            guard let spin = store.state["tool_settings"].array.first(where: { $0["id"].string == "gap_closing" }) else {
                throw HostFailure(message: "Auto select must expose Close gaps")
            }
            let rows: [(String, JSON, Double, Bool)] = [
                ("Brush opacity", store.catalog["opacity"], 0, true),
                ("Brush opacity", store.catalog["opacity"], 0.5, true),
                ("Brush opacity", store.catalog["opacity"], 1, true),
                ("Brush size", store.catalog["brush_size"], 24, true),
                ("Long brush diameter setting label", store.catalog["brush_size"], 2048, true),
                (spin["label"].string, spin["numeric"], 0, true),
                (spin["label"].string, spin["numeric"], 12, true),
                (spin["label"].string, spin["numeric"], 32, true),
                (spin["label"].string, spin["numeric"], 12, false),
                ("Brush opacity", store.catalog["opacity"], 0.5, false)]
            for theme in ["light", "dark"] {
                try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                    store.edit(["type": "set_theme", "theme": theme]) { error in
                        if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                    }
                }
                let geometry = NumberGeometry(), palette = EditorPalette(source: store.state["palette"])
                let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height),
                    styleMask: [.borderless], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                defer { window.contentView = nil; window.close() }
                window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                let content = HStack(alignment: .top, spacing: 8) {
                    ForEach(widths.indices, id: \.self) { column in
                        VStack(spacing: 0) {
                            ForEach(rows.indices, id: \.self) { row in
                                let item = rows[row]
                                NumberControl(store: store, label: item.0, value: item.2, control: item.1,
                                    identifier: "\(column)-\(row)") { _, completion in completion(nil) }
                                    .disabled(!item.3).frame(height: 64, alignment: .top)
                            }
                        }.frame(width: widths[column])
                    }
                }.padding(6).frame(width: width, height: height, alignment: .topLeading)
                    .coordinateSpace(name: "number-capture").environment(\.measureNumberControls, true)
                    .onPreferenceChange(NumberControlFrames.self) { geometry.frames = $0 }
                    .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                    .environment(\.controlActiveState, .active)
                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
                    .foregroundStyle(palette["text"]).tint(palette.accent).background(palette["panel"])
                let host = NSHostingView(rootView: content); window.contentView = host
                for _ in 0..<40 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
                guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
                    throw HostFailure(message: "No numeric control capture")
                }
                window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                let name = "\(platform)-\(theme)"
                try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
                let controls = try rows.map { item in JSON(["label": item.0, "control": item.1.raw,
                    "value": item.2, "enabled": item.3, "formatted": try store.resolveNumber(item.1, value: item.2, operation: ["type": "format"]).raw,
                    "minimum": try store.resolveNumber(item.1, value: item.1["min"].number, operation: ["type": "format"]).raw]).raw }
                let fixture = JSON(["schema": 1, "name": name, "width": width, "height": height,
                    "column_widths": widths, "scale": Double(bitmap.pixelsWide) / width, "theme": theme,
                    "palette": store.state["palette"].raw, "text_size": store.catalog["text_size_pt"].number * 4 / 3,
                    "rows": controls, "frames": geometry.frames.mapValues {
                        ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }])
                try fixture.encoded().write(to: directory.appendingPathComponent("native-\(name).json"), atomically: true, encoding: .utf8)
                precondition(geometry.frames.keys.filter { $0.hasSuffix(":root") }.count == 30)
                precondition(store.failure == nil, store.failure ?? "")
                print("Captured 30 numeric controls: \(name)")
            }
            try await checkEditing(store)
        }
    }

    @MainActor private static func checkEditing(_ store: EditorStore) async throws {
        func settle() async throws {
            for _ in 0..<20 { try await Task.sleep(for: .milliseconds(5)) }
        }
        func edit(_ action: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                store.edit(action) { error in
                    if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                }
            }
        }
        try await edit(["type": "invoke", "command": "auto_select"])
        let diameter = store.state["brush"]["diameter"].number
        func setting() -> JSON { store.state["tool_settings"].array.first { $0["id"].string == "gap_closing" } ?? JSON() }
        precondition(!setting().isNull, "Auto select must expose Close gaps")
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 226, height: 700),
            styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        let host = NSHostingView(rootView: ToolSettingsControls(store: store).frame(width: 226, height: 700))
        window.contentView = host
        try await settle(); host.layoutSubtreeIfNeeded()
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
        }
        guard let field = fields(host).first(where: { $0.accessibilityIdentifier() == "number-entry-tool-gap_closing" }) else {
            throw HostFailure(message: "Missing mounted Close gaps field")
        }
        func formatted() throws -> String {
            try store.resolveNumber(setting()["numeric"], value: setting()["value"].number, operation: ["type": "format"])["text"].string
        }
        func checkFormatted() throws {
            let expected = try formatted()
            precondition(field.stringValue == expected, "Idle spin fields must show shared units")
        }
        func draft(_ text: String) async throws {
            field.delegate?.controlTextDidBeginEditing?(Notification(name: NSControl.textDidBeginEditingNotification, object: field))
            try await settle()
            field.stringValue = text
            field.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: field))
            try await settle()
        }
        func key(_ selector: Selector) async throws {
            precondition(field.delegate?.control?(field, textView: NSTextView(), doCommandBy: selector) == true)
            try await settle()
        }
        try checkFormatted()
        try await draft("2 * (3 + 4) px")
        try await key(#selector(NSResponder.insertNewline(_:)))
        precondition(setting()["value"].number == 14); try checkFormatted()
        try await draft("2 * (")
        try await key(#selector(NSResponder.insertNewline(_:)))
        precondition(setting()["value"].number == 14 && field.stringValue == "2 * (", "Invalid expressions must preserve the draft and document settings")
        try await key(#selector(NSResponder.cancelOperation(_:)))
        try checkFormatted()
        try await key(#selector(NSResponder.moveUp(_:)))
        precondition(setting()["value"].number == 15)
        try await key(#selector(NSResponder.moveDown(_:)))
        precondition(setting()["value"].number == 14)
        try await draft("9 +")
        try await edit(["type": "set_tool_setting", "id": "gap_closing", "value": 26])
        try await settle()
        precondition(field.stringValue == "9 +", "Snapshots must preserve an unfinished field")
        try await key(#selector(NSResponder.cancelOperation(_:)))
        precondition(setting()["value"].number == 26); try checkFormatted()
        precondition(store.state["brush"]["diameter"].number == diameter && store.failure == nil)
        print("PASS: mounted numeric field delegates commit units/expressions, reject invalid input, step, preserve drafts and restore external values through the real editor")
    }
}
