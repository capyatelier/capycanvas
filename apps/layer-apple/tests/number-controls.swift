import AppKit
import SwiftUI

@MainActor private final class NumberGeometry {
    var frames: [String: CGRect] = [:]
    var choices: [String: CGRect] = [:]
}

@main struct NumberControlCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        let composing = NSTextView()
        composing.string = "한"
        composing.setMarkedText("글", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: 1, length: 0))
        precondition(composing.hasMarkedText())
        precondition(NativeTextContext.submittedText(in: composing) == "한글" && !composing.hasMarkedText(),
            "A delivered native submit must commit and read the final Korean syllable")
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
            try await store.apply(["type": "invoke", "command": "auto_select"])
            try await store.apply(["type": "invoke", "command": "edges_grow_shrink"])
            guard let spin = store.state["tool_settings"].array.first(where: { $0["id"].string == "edge_offset" }) else {
                throw HostFailure(message: "Auto select must expose Edge offset")
            }
            let rows: [(String, JSON, Double, Bool)] = [
                ("Brush opacity", store.catalog["opacity"], 0, true),
                ("Brush opacity", store.catalog["opacity"], 0.5, true),
                ("Brush opacity", store.catalog["opacity"], 1, true),
                ("Brush size", store.catalog["brush_size"], 24, true),
                ("Long brush diameter setting label", store.catalog["brush_size"], 1200.5, true),
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
                    .environment(\.compactPanelControls, true)
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
                    "labels": NativeTextContext.numericLabels(item.0, language: store.interfaceLanguage).raw,
                    "value": item.2, "enabled": item.3, "formatted": try store.resolveNumber(item.1, value: item.2, operation: ["type": "format"]).raw,
                    "minimum": try store.resolveNumber(item.1, value: item.1["min"].number, operation: ["type": "format"]).raw,
                    "maximum": try store.resolveNumber(item.1, value: item.1["max"].number, operation: ["type": "format"]).raw]).raw }
                let fixture = JSON(["schema": 1, "name": name, "width": width, "height": height,
                    "column_widths": widths, "scale": Double(bitmap.pixelsWide) / width, "theme": theme,
                    "palette": store.state["palette"].raw, "panel_metrics": store.catalog["panel_metrics"].raw,
                    "text_size": store.catalog["text_size_pt"].number * 4 / 3,
                    "rows": controls, "frames": geometry.frames.mapValues {
                        ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }])
                try fixture.encoded().write(to: directory.appendingPathComponent("native-\(name).json"), atomically: true, encoding: .utf8)
                precondition(geometry.frames.keys.filter { $0.hasSuffix(":root") }.count == 30)
                for column in widths.indices {
                    for row in [0, 1, 2, 3, 4, 9] {
                        let key = "\(column)-\(row)"
                        let root = geometry.frames[key + ":root"]!, track = geometry.frames[key + ":track"]!
                        let value = geometry.frames[key + ":value"]!
                        precondition(abs(root.height - store.panelMetrics.sliderHeight) < 0.5 && abs(track.height - 16) < 0.5)
                        precondition(abs(track.minX - root.minX - 36) < 0.5 && abs(root.maxX - track.maxX - 76) < 0.5)
                        precondition(value.width > 12 && abs(value.height - store.panelMetrics.height) < 0.5)
                        precondition(abs(value.maxX - root.maxX) < 0.5 && abs(value.maxY - root.maxY) < 0.5)
                        precondition(abs(track.maxY - root.maxY) < 0.5)
                        precondition(geometry.frames[key + ":minus"] == nil && geometry.frames[key + ":plus"] == nil)
                    }
                    for row in [5, 6, 7, 8] {
                        let key = "\(column)-\(row)", root = geometry.frames[key + ":root"]!
                        let entry = geometry.frames[key + ":entry"]!
                        let minus = geometry.frames[key + ":minus"]!, plus = geometry.frames[key + ":plus"]!
                        let control = store.panelMetrics.height
                        precondition(abs(root.height - control) < 0.5 && abs(entry.height - control) < 0.5)
                        precondition(abs(minus.width - control) < 0.5 && abs(minus.height - control) < 0.5)
                        precondition(abs(plus.width - control) < 0.5 && abs(plus.height - control) < 0.5)
                        precondition(abs(entry.maxX - minus.minX) < 0.5 && abs(minus.maxX - plus.minX) < 0.5)
                    }
                }
                for column in widths.indices {
                    precondition(geometry.frames["\(column)-2:value"]!.width > geometry.frames["\(column)-0:value"]!.width,
                        "Readouts must reserve the complete shaped number rather than a fixed value box")
                }
                precondition(store.failure == nil, store.failure ?? "")
                print("Captured 30 numeric controls: \(name)")
                try await checkPanelGeometry(store, directory: directory, platform: platform, theme: theme)
            }
            try await checkEditing(store)
        }
    }

    @MainActor private static func checkPanelGeometry(_ store: EditorStore, directory: URL, platform: UInt32, theme: String) async throws {
        try await store.apply(["type": "invoke", "command": "pencil"])
        for panelID in ["tool_settings", "properties"] {
            for width: CGFloat in [160, 226, 320] {
                let geometry = NumberGeometry(), palette = EditorPalette(source: store.state["palette"])
                let panel = JSON(["id": panelID, "controls": [["control": panelID, "visible_in_panel": true]]])
                let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: 700),
                    styleMask: [.borderless], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                defer { window.contentView = nil; window.close() }
                window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                let content = PanelControls(store: store, panel: panel, scrollable: false)
                    .frame(width: width, height: 700, alignment: .topLeading)
                    .coordinateSpace(name: "number-capture").environment(\.measureNumberControls, true)
                    .onPreferenceChange(NumberControlFrames.self) { geometry.frames = $0 }
                    .coordinateSpace(name: "choice-capture").environment(\.measureChoices, true)
                    .onPreferenceChange(ChoiceFrames.self) { geometry.choices = $0 }
                    .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                    .environment(\.controlActiveState, .active)
                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
                    .foregroundStyle(palette["text"]).tint(palette.accent).background(palette["panel"])
                let host = NSHostingView(rootView: content); window.contentView = host
                for _ in 0..<40 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
                let roots = geometry.frames.filter { $0.key.hasSuffix(":root") }.sorted { $0.value.minY < $1.value.minY }
                let groups = Dictionary(uniqueKeysWithValues: store.state["tool_settings"].array.map {
                    ("tool-" + $0["id"].string + ":root", $0["group"].string)
                })
                precondition(!roots.isEmpty, "Mounted \(panelID) must expose numeric controls")
                for (_, bounds) in roots {
                    precondition(abs(bounds.minX - 8) < 0.5 && abs(bounds.maxX - width + 8) < 0.5,
                        "\(panelID) must share the eight-point content rails: \(bounds)")
                }
                for pair in zip(roots, roots.dropFirst()) {
                    let heading = panelID == "tool_settings" && !(groups[pair.1.key] ?? "").isEmpty
                        && groups[pair.0.key] != groups[pair.1.key]
                    let spacing = store.panelMetrics.spacing + (heading ? store.panelMetrics.height : 0)
                    precondition(abs(pair.1.value.minY - pair.0.value.maxY - spacing) < 0.5,
                        "\(panelID) numeric row spacing must retain its shared section heading: \(pair.0.key)/\(pair.1.key)")
                }
                if panelID == "properties" {
                    let choice = geometry.choices["property-blend"]!, text = geometry.choices["property-blend:text"]!
                    precondition(abs(choice.height - store.panelMetrics.height) < 0.5 && abs(choice.maxX - width + 8) < 0.5,
                        "Property choices must share compact height and the right content rail")
                    precondition(abs(text.minX - choice.minX - 10) < 0.5,
                        "Property choice text must use the select disclosure inset")
                }
                guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
                    throw HostFailure(message: "No mounted panel capture")
                }
                window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                let name = "panel-\(panelID)-\(platform)-\(theme)-\(Int(width))"
                try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent(name + ".png"))
                try JSON(["panel": panelID, "platform": platform, "theme": theme, "width": width,
                    "frames": geometry.frames.mapValues {
                        ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] },
                    "choices": geometry.choices.mapValues {
                        ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }])
                    .encoded().write(to: directory.appendingPathComponent(name + ".json"), atomically: true, encoding: .utf8)
            }
        }
    }

    @MainActor private static func checkEditing(_ store: EditorStore) async throws {
        func settle() async throws {
            for _ in 0..<20 { try await Task.sleep(for: .milliseconds(5)) }
        }
        try await store.apply(["type": "invoke", "command": "auto_select"])
        try await store.apply(["type": "invoke", "command": "edges_grow_shrink"])
        let diameter = store.state["brush"]["diameter"].number
        let size = store.catalog["brush_size"]
        let scrub = try store.resolveNumber(size, value: 1200, operation: ["type": "scrub", "origin": 1200, "pixels": 2])
        precondition(scrub["value"].number == 1201 && scrub["text"].string == "1201 px")
        let settled = try store.resolveNumber(size, value: scrub["value"].number, operation: ["type": "format"])
        precondition(settled["text"].string == "1201 px")
        let operations: [[String: Any]] = [["type": "format"], ["type": "expression", "text": "1200.5 px"]]
        for operation in operations {
            let fractional = try store.resolveNumber(size, value: 1200.5, operation: operation)
            precondition(fractional["value"].number == 1200.5 && fractional["text"].string == "1200.5 px"
                && fractional["edit"].string == "1200.5")
        }
        func setting() -> JSON { store.state["tool_settings"].array.first { $0["id"].string == "edge_offset" } ?? JSON() }
        precondition(!setting().isNull, "Auto select must expose Edge offset")
        NSApp.setActivationPolicy(.accessory)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 226, height: 700),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close(); NSApp.setActivationPolicy(.prohibited) }
        let host = NSHostingView(rootView: ToolSettingsControls(store: store).frame(width: 226, height: 700))
        window.contentView = host
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await settle(); host.layoutSubtreeIfNeeded()
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
        }
        guard let field = fields(host).first(where: { $0.accessibilityIdentifier() == "number-entry-tool-edge_offset" }) else {
            throw HostFailure(message: "Missing mounted Edge offset field")
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
            let editor = NSTextView(); editor.string = field.stringValue
            precondition(field.delegate?.control?(field, textView: editor, doCommandBy: selector) == true)
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
        try await store.apply(["type": "set_tool_setting", "id": "edge_offset", "value": 26])
        try await settle()
        precondition(field.stringValue == "9 +", "Snapshots must preserve an unfinished field")
        try await key(#selector(NSResponder.cancelOperation(_:)))
        precondition(setting()["value"].number == 26); try checkFormatted()
        precondition(store.state["brush"]["diameter"].number == diameter && store.failure == nil)
        try await draft("19 + ")
        precondition(window.makeFirstResponder(field))
        guard let editor = field.currentEditor() as? NSTextView else { throw HostFailure(message: "Missing native numeric field editor") }
        editor.string = "19 + "
        editor.setMarkedText("8", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: 5, length: 0))
        precondition(editor.hasMarkedText() && editor.string == "19 + 8")
        precondition(field.delegate?.control?(field, textView: editor, doCommandBy: #selector(NSResponder.insertNewline(_:))) == true,
            "A delivered native Return must submit after committing marked text")
        precondition(!editor.hasMarkedText())
        try await settle()
        precondition(setting()["value"].number == 27, "Return must submit the full native value instead of the earlier draft")
        try checkFormatted()
        print("PASS: mounted numeric field delegates commit units/expressions, reject invalid input, step, preserve drafts and restore external values through the real editor")
    }
}
