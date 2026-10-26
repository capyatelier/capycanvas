import AppKit
import SwiftUI
import QuartzCore

@MainActor private final class PropertySliderGeometry {
    var frames: [String: CGRect] = [:]
    var choices: [String: CGRect] = [:]
}

/// Native slider contacts through the real property views and serial owner.
/// Temporary windows and in-memory documents; no artist storage.
@main final class PropertySliderChecks: NativeWorkspaceInputFixture {
    @MainActor private static func keyboardArrival() throws {
        let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 80, height: 80),
            styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        let input = ControlInput.InputView(frame: CGRect(x: 0, y: 0, width: 80, height: 80))
        var activations = [String]()
        input.contact = { _ in }
        input.key = { name, pressed, _ in
            guard ["Space", "Enter"].contains(name) else { return false }
            if !pressed { activations.append(name) }
            return true
        }
        window.contentView = input
        func key(_ text: String, _ code: UInt16) throws {
            for type: NSEvent.EventType in [.keyDown, .keyUp] {
                guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, characters: text,
                    charactersIgnoringModifiers: text, isARepeat: false, keyCode: code) else {
                    throw HostFailure(message: "Missing focus regression key")
                }
                if type == .keyDown { input.keyDown(with: event) } else { input.keyUp(with: event) }
            }
        }
        guard let down = NSEvent.mouseEvent(with: .leftMouseDown, location: CGPoint(x: 20, y: 20), modifierFlags: [],
            timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
            throw HostFailure(message: "Missing focus regression contact")
        }
        input.mouseDown(with: down)
        try key(" ", 49)
        try require(activations.isEmpty, "Pointer arrival must leave Space for the canvas")
        try key("\r", 36)
        try require(activations == ["Enter"], "Enter activates a pointer-focused readout")
        window.makeFirstResponder(nil); window.makeFirstResponder(input)
        try key(" ", 49)
        try require(activations == ["Enter", "Space"], "Keyboard arrival enables Space activation")
    }
    @MainActor static func run() async throws {
        try keyboardArrival()
        let filter = ProcessInfo.processInfo.environment["CAPY_PROPERTY_CASE"] ?? ""
        let cases = [("", "brush-size"), ("", "brush-opacity"), ("", "tool-document"),
            ("curves", "point"), ("", "opacity"), ("", "layer-opacity"),
            ("brightness_contrast", "brightness"),
            ("gradient_map", "position"), ("gradient_map", "stop-draft"), ("", "light")]
            .filter { filter.isEmpty || $0.1.hasPrefix(filter) }
        try require(!cases.isEmpty, "No property cases match \(filter)")
        for platform: UInt32 in [0, 1] {
            for (effect, mode) in cases {
                let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
                try await wait("Property owner startup") { !store.state["layer_properties"]["controls"].array.isEmpty }
                func action(_ value: [String: Any]) async throws {
                    try await store.apply(value)
                    try await drain()
                }
                if !effect.isEmpty {
                    try await action(["type": "effect", "action": ["op": "insert", "effect": effect]])
                }
                let toolControl = mode == "tool-document"
                if toolControl { try await action(["type": "invoke", "command": "auto_select"]) }
                if mode == "light" {
                    func lit() -> Bool { store.state["layer_properties"]["controls"].array.contains { $0["key"].string == "light" } }
                    for row in store.state["layers"].array where !lit() { try await action(["type": "select_layer", "id": row["id"].uint]) }
                    try require(lit(), "The Paper layer must expose its Light dial")
                    try await action(["type": "effect", "action": ["op": "set",
                        "layer": store.state["layer_properties"]["layer"].uint,
                        "key": "relief", "value": ["kind": "number", "value": 0]]])
                    let flatControls = store.state["layer_properties"]["controls"].array
                    try require(flatControls.first { $0["key"].string == "relief" }!["value"]["value"].number == 0
                        && flatControls.first { $0["key"].string == "light" }!["disabled"].bool,
                        "The Light fixture must publish zero relief and a disabled dial before mounting")
                }
                let layer = store.state["layer_properties"]["layer"].uint
                let brushControl = mode == "brush-size" || mode == "brush-opacity"
                let brushControlID = mode == "brush-size" ? "brush_size" : "brush_opacity"
                if brushControl {
                    try await action(["type": "select_brush", "id": 1])
                    try require(store.state["brush"]["preset"].uint == 1,
                        "The brush-control fixture must start with the published GPen preset")
                    try await action(["type": "customize", "action": ["type": "set_control_visible",
                        "panel": "sizes", "control": brushControlID, "visible": true]])
                }
                let brushPanel = store.panel("sizes")
                let brushItems = brushPanel["controls"].array.filter { $0["control"].string == brushControlID }
                if brushControl {
                    try require(brushItems.count == 1 && brushItems[0]["visible_in_panel"].bool && !brushItems[0]["label"].string.isEmpty,
                        "The shared Sizes panel must publish its visible brush control and label")
                }
                let defaultValues = JSON(store.state["layer_properties"]["controls"].array.map { $0["value"].raw }).stableKey
                let key = effect == "curves" ? "rgb" : effect == "gradient_map" ? "gradient"
                    : effect == "brightness_contrast" ? "brightness" : mode == "light" ? "light" : "opacity"
                let identifier = toolControl ? "tool-tolerance" : brushControl ? brushItems[0]["label"].string
                    : mode == "layer-opacity" ? mode : effect == "gradient_map"
                    ? "gradient-strip"
                    : "property-" + key
                func value() -> Double {
                    if toolControl { return store.state["tool_settings"].array.first { $0["id"].string == "tolerance" }!["value"].number }
                    if brushControl { return store.state["brush"][mode == "brush-size" ? "diameter" : "opacity"].number }
                    let value = store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"]
                    if effect == "gradient_map" { return value["stops"][1]["position"].number }
                    return value.number
                }
                let geometry = PropertySliderGeometry()
                let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 300, height: 500),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                defer { window.contentView = nil; window.close() }
                let content = AnyView(Group {
                    if toolControl { ToolSettingsControls(store: store) }
                    else if brushControl {
                        PanelControls(store: store, panel: brushPanel.replacing("controls", with: JSON(brushItems.map(\.raw))),
                            scrollable: false, measureForWorkspace: false)
                    } else if mode == "layer-opacity" { LayerOpacityField(store: store) }
                    else { LayerPropertiesPanel(store: store) }
                }.padding(6).frame(width: 300, height: 500, alignment: .topLeading)
                    .environment(\.compactPanelControls, true)
                    .coordinateSpace(name: "number-capture").environment(\.measureNumberControls, true)
                    .onPreferenceChange(NumberControlFrames.self) { geometry.frames = $0 }
                    .coordinateSpace(name: "choice-capture").environment(\.measureChoices, true)
                    .onPreferenceChange(ChoiceFrames.self) { geometry.choices = $0 }
                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3)))
                let host = NSHostingView(rootView: content)
                window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                try await drain(0.3)
                func click(_ point: CGPoint) async throws {
                    for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                        try event(type, at: point, marker: host, number: 1); try await drain()
                    }
                }
                func draft(_ identifier: String, _ text: String) async throws -> (NSTextField, any NSTextFieldDelegate) {
                    guard let valueFrame = geometry.frames[identifier + ":value"] else {
                        throw HostFailure(message: "Missing property readout before target switch")
                    }
                    try await click(CGPoint(x: valueFrame.midX, y: valueFrame.midY))
                    func fields(_ view: NSView) -> [NSTextField] {
                        (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
                    }
                    guard let field = fields(host).first(where: { $0.accessibilityIdentifier() == "number-entry-" + identifier }),
                        let delegate = field.delegate else {
                        throw HostFailure(message: "Missing mounted property field before target switch")
                    }
                    delegate.controlTextDidBeginEditing?(Notification(name: NSControl.textDidBeginEditingNotification, object: field))
                    guard let editor = field.currentEditor() as? NSTextView else {
                        throw HostFailure(message: "The mounted number must own its AppKit field editor")
                    }
                    editor.insertText(text, replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
                    try await drain()
                    try require(field.stringValue == text, "The property draft must remain unfinished")
                    guard let entryFrame = geometry.frames[identifier + ":entry"] else {
                        throw HostFailure(message: "Missing mounted property editor geometry")
                    }
                    let control = store.panelMetrics.height
                    try require(abs(valueFrame.height - control) < 0.5 && abs(entryFrame.height - control) < 0.5,
                        "Panel readouts and editors must share the panel control height")
                    try require(abs(valueFrame.maxY - entryFrame.maxY) < 0.5,
                        "Entering a panel number must preserve its bottom alignment")
                    if let root = geometry.frames[identifier + ":root"],
                        let track = geometry.frames[identifier + ":track"], abs(root.height - store.panelMetrics.sliderHeight) < 0.5 && abs(track.height - 16) < 0.5 {
                        try require(abs(entryFrame.width - 68) < 0.5 && abs(entryFrame.maxX - root.maxX) < 0.5
                            && abs(entryFrame.maxY - root.maxY) < 0.5 && entryFrame.minX - track.maxX >= 7.5,
                            "Panel editors must keep the upstream width, bottom rail and track clearance")
                    }
                    return (field, delegate)
                }
                func commit(_ field: NSTextField, _ delegate: any NSTextFieldDelegate) async throws {
                    try require(delegate.control?(field, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:))) == true,
                        "The retained native field must handle its commit callback")
                    try await drain()
                }
                if toolControl {
                    let (field, delegate) = try await draft(identifier, "37 %")
                    let epoch = store.state["document_file"]["epoch"].uint
                    let target = store.state["layer_tools"]["editing_layer"]["id"].uint
                    // Use the production document replacement path with an
                    // offscreen Metal surface and a supplied New Drawing size.
                    _ = attachSurface(store, CGSize(width: 128, height: 128))
                    try await action(["type": "set_tool_setting", "id": "tolerance", "value": 0.23])
                    try require(field.isDescendant(of: host) && field.stringValue == "37 %",
                        "An ordinary tool-value update must preserve its active draft")
                    store.projectFiles = ProjectFiles(store: store, dialogs: .init(
                        open: { _, done in done([]) }, save: { _, _, completed in completed(nil) },
                        create: { _, completed in completed(newSheet([256, 256], space: "Srgb", depth: "U8")) }))
                    store.invoke("new_document")
                    let deadline = Date().addingTimeInterval(30)
                    while store.state["document_file"]["epoch"].uint == epoch || store.projectFiles.busy {
                        try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
                        try require(Date() < deadline, "Tool-settings document replacement timed out")
                        try await drain(0.01)
                    }
                    try await action(["type": "invoke", "command": "auto_select"])
                    try require(store.state["layer_tools"]["editing_layer"]["id"].uint == target,
                        "The replacement must reuse the previous numeric layer ID")
                    let replacement = value()
                    try await commit(field, delegate)
                    try require(abs(value() - replacement) < 0.0001,
                        "A retired tool draft must not edit another document after the same tool is selected: expected \(replacement), found \(value())")
                    try require(!field.isDescendant(of: host), "Document replacement must discard the original field")
                    let (current, currentDelegate) = try await draft(identifier, "37 %")
                    try await commit(current, currentDelegate)
                    try require(abs(value() - 0.37) < 0.0001 && store.failure == nil,
                        "The replacement document's own tool field must accept edits")
                    note("PASS: platform \(platform), tool draft rejects reused targets across documents and preserves ordinary edits")
                    continue
                }
                if brushControl {
                    if mode == "brush-size" {
                        try await action(["type": "set_brush_size", "value": 1200.5])
                        guard let readout = geometry.frames[identifier + ":value"] else {
                            throw HostFailure(message: "Missing mounted brush readout")
                        }
                        let origin = CGPoint(x: readout.midX, y: readout.midY)
                        let moved = CGPoint(x: origin.x, y: origin.y - 8)
                        try event(.leftMouseDown, at: origin, marker: host, number: 1); try await drain()
                        try event(.leftMouseDragged, at: moved, marker: host, number: 1); try await drain()
                        try require(value() == 1203, "A mounted large-pixel scrub must publish an integer preview")
                        try NativeWorkspaceInputFixture.key("\u{1B}", code: 53, window: window); try await drain()
                        try require(value() == 1200.5, "Escape must restore the held brush scrub's fractional origin")
                        try event(.leftMouseUp, at: moved, marker: host, number: 1); try await drain()
                        try require(value() == 1200.5, "Release after cancellation must preserve the original brush size")
                    }
                    let text = mode == "brush-size" ? "37 px" : "37 %"
                    let (field, delegate) = try await draft(identifier, text)
                    guard let entry = geometry.frames[identifier + ":entry"],
                        let root = geometry.frames[identifier + ":root"] else {
                        throw HostFailure(message: "Missing mounted brush entry geometry")
                    }
                    try require(abs(entry.width - 68) < 0.5 && abs(entry.height - store.panelMetrics.height) < 0.5
                        && abs(entry.maxX - root.maxX) < 0.5 && abs(entry.maxY - root.maxY) < 0.5,
                        "Mounted panel entries must occupy 68 points by the panel control height at the bottom right")
                    let presetBeforeSwitch = store.state["brush"]["preset"].uint
                    try await action(["type": "select_brush", "id": 2])
                    let presetAfterSwitch = store.state["brush"]["preset"].uint
                    try require(presetBeforeSwitch == 1 && presetAfterSwitch == 2 && presetBeforeSwitch != presetAfterSwitch,
                        "The retained brush draft must face a different published preset before its commit")
                    let replacement = value()
                    try await commit(field, delegate)
                    try require(abs(value() - replacement) < 0.0001,
                        "A retired \(identifier) draft must not edit the replacement brush: expected \(replacement), found \(value())")
                    try require(!field.isDescendant(of: host), "Changing brush must discard its old numeric draft")
                    let (current, currentDelegate) = try await draft(identifier, text)
                    let actionType = mode == "brush-size" ? "set_brush_size" : "set_brush_opacity"
                    let external = mode == "brush-size" ? 23.0 : 0.23
                    try await action(["type": actionType, "value": external])
                    try require(current.isDescendant(of: host) && current.stringValue == text,
                        "An ordinary brush-value update must preserve its active draft")
                    try await commit(current, currentDelegate)
                    let accepted = mode == "brush-size" ? 37.0 : 0.37
                    try require(abs(value() - accepted) < 0.0001,
                        "The replacement brush's own field must still accept edits: expected \(accepted), found \(value())")
                    note("PASS: platform \(platform), \(identifier), brush switch rejects retired edits while ordinary updates preserve drafts")
                    continue
                }
                if effect == "curves" {
                    func points() -> JSON {
                        store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"]
                    }
                    let original = points().stableKey
                    let top = geometry.choices["properties-page"]!.maxY + 6
                    try await click(CGPoint(x: 150, y: top + 50))
                    let inserted = points().stableKey
                    try require(points().array.count == 3 && inserted != original, "A native plot click must insert a curve point")
                    func remove() async throws { try NativeWorkspaceInputFixture.key("\u{7F}", code: 51, window: window); try await drain(0.2) }
                    try await remove()
                    try require(points().stableKey == original, "Delete must remove a new curve point without another selection tap")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(points().stableKey == inserted, "Undo must restore the removed curve point")
                    try await click(CGPoint(x: 150, y: top + 50))
                    try await remove()
                    try require(points().stableKey == original, "A restored curve point must be selectable for removal")
                    try await action(["type": "invoke", "command": "undo"])
                    try await action(["type": "invoke", "command": "redo"])
                    try require(points().stableKey == original, "Curve removal must retain one-step Undo/Redo")
                    try require(store.failure == nil, store.failure ?? "")
                    note("PASS: platform \(platform), native curve insertion/selection/removal and history")
                    continue
                }
                if mode == "light" {
                    func dial() throws -> CGRect {
                        guard let bounds = geometry.frames[identifier + ":dial"], let root = geometry.frames[identifier + ":root"] else {
                            throw HostFailure(message: "Missing native Light dial")
                        }
                        let diameter = store.panelMetrics.sliderHeight - 2 * store.panelMetrics.spacing
                        try require(abs(root.height - store.panelMetrics.sliderHeight) < 0.5 && abs(bounds.width - diameter) < 0.5
                            && abs(bounds.height - diameter) < 0.5,
                            "The Light dial must be a slider row's height less its control spacing: root=\(root), dial=\(bounds)")
                        try require(abs(root.maxX - 76 - bounds.maxX) < 0.5 && abs(bounds.midY - root.midY) < 0.5,
                            "The Light dial must end where slider tracks end: root=\(root), dial=\(bounds)")
                        return bounds
                    }
                    let original = value()
                    let flat = try dial()
                    try event(.leftMouseDown, at: CGPoint(x: flat.maxX - 2, y: flat.midY), marker: host, number: 1); try await drain()
                    try event(.leftMouseDragged, at: CGPoint(x: flat.midX, y: flat.midY + 80), marker: host, number: 1); try await drain()
                    try event(.leftMouseUp, at: CGPoint(x: flat.midX, y: flat.midY + 80), marker: host, number: 1); try await drain()
                    try require(value() == original && store.failure == nil, "A flat paper's Light dial must refuse input: \(value())")
                    try await action(["type": "effect", "action": ["op": "set", "layer": layer, "key": "relief", "value": ["kind": "number", "value": 150]]])
                    try require(!store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["disabled"].bool,
                        "Texture visibility must enable Light")
                    let turning = try dial()
                    let far = CGPoint(x: turning.midX, y: turning.midY + 80)
                    try event(.leftMouseDown, at: CGPoint(x: turning.maxX - 2, y: turning.midY), marker: host, number: 1); try await drain()
                    try require(abs(value()) < 0.0001, "A press must point the light where pressed: \(value())")
                    try event(.leftMouseDragged, at: far, marker: host, number: 1); try await drain()
                    try event(.leftMouseUp, at: far, marker: host, number: 1); try await drain()
                    try require(abs(value() - 90) < 0.0001, "A drag outside the dial must keep turning it: \(value())")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(value() == original, "One Undo must restore the whole dial turn: \(value())")
                    try await action(["type": "invoke", "command": "redo"])
                    try require(abs(value() - 90) < 0.0001, "One Redo must restore the dial turn: \(value())")
                    try require(store.failure == nil, store.failure ?? "")
                    note("PASS: platform \(platform), Light dial geometry, flat-paper refusal, pointing, outside drag and one-step history")
                    continue
                }
                let markerY = (geometry.frames["gradient-strip:track"]?.minY ?? 0) + 38
                if effect == "gradient_map" {
                    // Add the stop through the real view. Its controls must target
                    // the inserted stop immediately, without a second selection tap.
                    try await click(CGPoint(x: 150, y: markerY))
                    let stops = store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"]["stops"].array
                    try require(stops.count == 3 && abs(stops[1]["position"].number - 0.5) < 0.0001,
                        "Native gradient insertion must add the middle stop")
                }
                if effect == "gradient_map" && mode == "stop-draft" {
                    func values() -> String { JSON(store.state["layer_properties"]["controls"].array.map { $0["value"].raw }).stableKey }
                    let identifier = "gradient-position"
                    try await click(CGPoint(x: 150, y: markerY))
                    let (field, delegate) = try await draft(identifier, "37 %")
                    try await click(CGPoint(x: 12, y: markerY))
                    let accepted = values()
                    try await commit(field, delegate)
                    try require(values() == accepted,
                        "A retired gradient field must not change either stop after selection switches")
                    note("PASS: platform \(platform), gradient stop selection rejects a retired \(mode) field")

                    try await click(CGPoint(x: 150, y: markerY))
                    let (insertionField, insertionDelegate) = try await draft(identifier, "37 %")
                    try await click(CGPoint(x: 81, y: markerY))
                    let inserted = values()
                    let stops = store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"]["stops"].array
                    try require(stops.count == 4 && abs(stops[1]["position"].number - 0.25) < 0.0001,
                        "The native plot must insert a new stop at the old selected index")
                    try require(geometry.frames[identifier + ":entry"] == nil,
                        "Insertion must discard the old field's visible draft")
                    try await commit(insertionField, insertionDelegate)
                    try require(values() == inserted,
                        "A previous stop's unfinished field must not edit a new stop reusing its index")
                    note("PASS: platform \(platform), gradient insertion retires the \(mode) field even when its index is reused")

                    let (historyField, historyDelegate) = try await draft(identifier, "37 %")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(values() == accepted,
                        "One Undo must remove the inserted stop and restore the original gradient")
                    try await commit(historyField, historyDelegate)
                    try require(values() == accepted,
                        "A field from the undone stop must not edit its replacement")
                    try await action(["type": "invoke", "command": "redo"])
                    try require(values() == inserted,
                        "An ignored callback must preserve insertion Redo")
                    note("PASS: platform \(platform), gradient Undo/Redo retires the \(mode) draft and retains history")

                    let (removedField, removedDelegate) = try await draft(identifier, "37 %")
                    let remove = geometry.frames["gradient-remove:root"]!, reset = geometry.frames["gradient-reset:root"]!
                    try await click(CGPoint(x: remove.midX, y: remove.midY))
                    try require(values() == accepted,
                        "Remove stop must remove the selected insertion")
                    try await commit(removedField, removedDelegate)
                    try require(values() == accepted,
                        "A removed stop's field must not change the remaining gradient")
                    try await click(CGPoint(x: 150, y: markerY))
                    let (resetField, resetDelegate) = try await draft(identifier, "37 %")
                    try await click(CGPoint(x: reset.midX, y: reset.midY))
                    try require(values() == defaultValues,
                        "The native Reset button must restore the shared gradient defaults")
                    try await commit(resetField, resetDelegate)
                    try require(values() == defaultValues,
                        "Reset must discard an unfinished position draft")
                    note("PASS: platform \(platform), gradient Remove and Reset discard unfinished position drafts")
                    try require(store.failure == nil, store.failure ?? "")
                    continue
                }
                guard let track = geometry.frames[identifier + ":track"] else {
                    throw HostFailure(message: "Missing native property slider")
                }
                let original = value()
                func contact(_ type: NSEvent.EventType, _ fraction: CGFloat) async throws {
                    let point = effect == "gradient_map"
                        ? CGPoint(x: track.minX + 6 + (track.width - 12) * (0.5 + (fraction - 0.2) / 2), y: markerY)
                        : CGPoint(x: track.minX + track.width * fraction, y: track.midY)
                    try event(type, at: point, marker: host, number: 1)
                    try await drain()
                }
                try await contact(.leftMouseDown, 0.2)
                for fraction: CGFloat in [0.35, 0.5, 0.65, 0.8] { try await contact(.leftMouseDragged, fraction) }
                try await contact(.leftMouseUp, 0.8)
                let edited = value()
                try require(abs(edited - original) > 0.01, "The native drag must change \(key)")
                try await action(["type": "invoke", "command": "undo"])
                try require(abs(value() - original) < 0.0001,
                    "One Undo must restore the complete \(key) drag: original=\(original), edited=\(edited), undo=\(value())")
                try await action(["type": "invoke", "command": "redo"])
                try require(abs(value() - edited) < 0.0001, "One Redo must restore the complete slider edit")
                // Removing the active native control cancels its preview without
                // consuming the Redo established by the previous completed drag.
                try await action(["type": "invoke", "command": "undo"])
                try await contact(.leftMouseDown, 0.2); try await contact(.leftMouseDragged, 0.6)
                try require(abs(value() - original) > 0.01, "Cancellation needs a real preview")
                host.rootView = AnyView(Color.clear.frame(width: 300, height: 500))
                try await drain(0.2)
                try require(abs(value() - original) < 0.0001, "Removing a slider must restore its original value")
                try await contact(.leftMouseUp, 0.6)
                try await action(["type": "invoke", "command": "redo"])
                try require(abs(value() - edited) < 0.0001, "Cancellation and late release must preserve Redo")
                host.rootView = content; try await drain(0.2)
                try await action(["type": "layer", "action": ["op": "lock", "id": layer, "value": true]])
                try await contact(.leftMouseDown, 0.2); try await contact(.leftMouseDragged, 0.6)
                try await contact(.leftMouseUp, 0.6)
                try require(abs(value() - edited) < 0.0001, "Locked slider input must not change the property")
                try await action(["type": "invoke", "command": "undo"])
                try require(store.state["layer_properties"]["enabled"].bool, "Locked input must not add an Undo step")
                if effect.isEmpty && ["opacity", "layer-opacity"].contains(mode) || mode == "brightness" {
                    let (field, delegate) = try await draft(identifier, mode == "brightness" ? "0.37" : "37 %")
                    try await action(["type": "invoke", "command": "add_layer"])
                    let replacement = store.state["layer_properties"]["layer"].uint
                    try require(replacement != layer, "Add layer must change the property target")
                    let replacementValues = store.state["layer_properties"]["controls"].stableKey
                    // Retain the old native delegate to deliver a delayed commit
                    // after SwiftUI has retired its field. This is a callback
                    // lifetime check, not a hardware-key delivery assertion.
                    try await commit(field, delegate)
                    try require(store.state["layer_properties"]["controls"].stableKey == replacementValues,
                        "A retired property field must not edit the replacement layer")
                    try await action(["type": "select_layer", "id": layer])
                    try require(abs(value() - edited) < 0.0001,
                        "A retired property draft must not edit its former layer: expected \(edited), found \(value())")
                    let wrongEpoch = store.state["document_file"]["epoch"].uint &+ 1
                    let error = await withCheckedContinuation { done in
                        store.effect(layer, epoch: wrongEpoch, key: key,
                            action: ["op": "set", "value": ["kind": "number", "value": 0.37]]) { done.resume(returning: $0) }
                    }
                    try await drain()
                    try require(error == nil && abs(value() - edited) < 0.0001,
                        "A property callback for another document epoch must be ignored")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(!store.state["layers"].array.contains { $0["id"].uint == replacement },
                        "Ignored field callbacks must not add history after Add layer")
                    try require(abs(value() - edited) < 0.0001, "Undo must retain the former layer's accepted value")
                    note("PASS: platform \(platform), \(identifier), retired field and document-epoch rejection preserve values and history")
                }
                try require(store.failure == nil, store.failure ?? "")
                note("PASS: platform \(platform), \(effect.isEmpty ? "paint" : effect)/\(mode), native slider, one-step history and cancellation")
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
