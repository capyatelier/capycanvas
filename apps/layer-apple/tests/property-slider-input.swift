import AppKit
import SwiftUI

@MainActor private final class PropertySliderGeometry {
    var frames: [String: CGRect] = [:]
    var choices: [String: CGRect] = [:]
}

/// Native slider contacts through the real property views and serial owner.
/// Temporary windows and in-memory documents; no renderer or artist storage.
@main final class PropertySliderChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        for platform: UInt32 in [0, 1] {
            for (effect, mode) in [("curves", "point"), ("", "opacity"), ("paper", "opacity"), ("", "layer-opacity"),
                ("brightness_contrast", "brightness"), ("split_tone", "red"),
                ("gradient_map", "position"), ("gradient_map", "opacity"), ("gradient_map", "red")] {
                let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
                let deadline = Date().addingTimeInterval(15)
                while store.state["layer_properties"]["controls"].array.isEmpty {
                    try require(Date() < deadline, "Property owner startup timed out")
                    try await drain(0.01)
                }
                func action(_ value: [String: Any]) async throws {
                    try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                        store.edit(value) { error in
                            if let error { done.resume(throwing: HostFailure(message: error)) }
                            else { done.resume() }
                        }
                    }
                    try await drain()
                }
                if effect == "paper" {
                    guard let paper = store.state["layers"].array.first(where: { !$0["can_drop_below"].bool }) else {
                        throw HostFailure(message: "Missing Paper layer")
                    }
                    try await action(["type": "select_layer", "id": paper["id"].raw])
                } else if !effect.isEmpty {
                    try await action(["type": "effect", "action": ["op": "insert", "effect": effect]])
                }
                let layer = store.state["layer_properties"]["layer"].uint
                let defaultControls = store.state["layer_properties"]["controls"].stableKey
                let key = effect == "curves" ? "curve_0" : effect == "gradient_map" ? "gradient" : effect == "split_tone" ? "shadows"
                    : effect == "brightness_contrast" ? "brightness" : "opacity"
                let identifier = mode == "layer-opacity" ? mode : effect == "gradient_map"
                    ? (mode == "red" ? "gradient-stop-rgba-0" : "gradient-" + mode)
                    : "property-" + key + (mode == "red" ? "-rgba-0" : "")
                func value() -> Double {
                    let value = store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"]
                    if effect == "gradient_map" {
                        return mode == "position" ? value[1]["position"].number : value[1]["color"][mode == "red" ? 0 : 3].number
                    }
                    return mode == "red" ? value[0].number : value.number
                }
                let geometry = PropertySliderGeometry()
                let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 300, height: 500),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                defer { window.contentView = nil; window.close() }
                let content = AnyView(Group {
                    if mode == "layer-opacity" { LayerOpacityField(store: store) }
                    else { LayerPropertiesPanel(store: store) }
                }.padding(6).frame(width: 300, height: 500, alignment: .topLeading)
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
                    field.stringValue = text
                    delegate.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: field))
                    try await drain()
                    try require(field.stringValue == text, "The property draft must remain unfinished")
                    return (field, delegate)
                }
                func commit(_ field: NSTextField, _ delegate: any NSTextFieldDelegate) async throws {
                    try require(delegate.control?(field, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:))) == true,
                        "The retained native field must handle its commit callback")
                    try await drain()
                }
                if effect == "curves" {
                    func points() -> JSON {
                        store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"]
                    }
                    let original = points().stableKey
                    let top = geometry.choices["property-channel"]!.maxY + 6
                    try await click(CGPoint(x: 150, y: top + 50))
                    let inserted = points().stableKey
                    try require(points().array.count == 3 && inserted != original, "A native plot click must insert a curve point")
                    let remove = CGPoint(x: 45, y: top + 200 + 6 + 10)
                    try await click(remove)
                    try require(points().stableKey == original, "Remove must act on a new curve point without another selection tap")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(points().stableKey == inserted, "Undo must restore the removed curve point")
                    try await click(remove)
                    try require(points().stableKey == original, "A restored curve point must be selected for removal")
                    try await action(["type": "invoke", "command": "undo"])
                    try await action(["type": "invoke", "command": "redo"])
                    try require(points().stableKey == original, "Curve removal must retain one-step Undo/Redo")
                    try require(store.failure == nil, store.failure ?? "")
                    note("PASS: platform \(platform), native curve insertion/selection/removal and history")
                    continue
                }
                if effect == "gradient_map" {
                    let position = geometry.frames["gradient-position:root"]!
                    // Add the stop through the real view. Its controls must target
                    // the inserted stop immediately, without a second selection tap.
                    try await click(CGPoint(x: 150, y: position.minY - 6 - 52 + 43))
                    let stops = store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"].array
                    try require(stops.count == 3 && abs(stops[1]["position"].number - 0.5) < 0.0001,
                        "Native gradient insertion must add the middle stop")
                }
                if mode == "red" {
                    let swatchY = effect == "gradient_map" ? geometry.frames["gradient-position:root"]!.maxY + 6 + 14
                        : geometry.frames["property-balance:root"]!.minY - 6 - 28 - 6 - 14
                    try await click(CGPoint(x: 300 - 6 - 24, y: swatchY))
                }
                guard let track = geometry.frames[identifier + ":track"] else {
                    throw HostFailure(message: "Missing native property slider")
                }
                let original = value()
                func contact(_ type: NSEvent.EventType, _ fraction: CGFloat) async throws {
                    try event(type, at: CGPoint(x: track.minX + track.width * fraction, y: track.midY), marker: host, number: 1)
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
                if effect != "paper" {
                    try await action(["type": "layer", "action": ["op": "lock", "id": layer, "value": true]])
                    try await contact(.leftMouseDown, 0.2); try await contact(.leftMouseDragged, 0.6)
                    try await contact(.leftMouseUp, 0.6)
                    try require(abs(value() - edited) < 0.0001, "Locked slider input must not change the property")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(store.state["layer_properties"]["enabled"].bool, "Locked input must not add an Undo step")
                }
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
                if effect == "gradient_map" && ["opacity", "red"].contains(mode) {
                    let markerY = geometry.frames["gradient-position:root"]!.minY - 6 - 52 + 43
                    try await click(CGPoint(x: 150, y: markerY))
                    if mode == "red" {
                        try await click(CGPoint(x: 300 - 6 - 24, y: geometry.frames["gradient-position:root"]!.maxY + 6 + 14))
                    }
                    let (field, delegate) = try await draft(identifier, "37 %")
                    try await click(CGPoint(x: 12, y: markerY))
                    let accepted = store.state["layer_properties"]["controls"].stableKey
                    try await commit(field, delegate)
                    try require(store.state["layer_properties"]["controls"].stableKey == accepted,
                        "A retired gradient field must not change either stop after selection switches")
                    note("PASS: platform \(platform), gradient stop selection rejects a retired \(mode) field")

                    try await click(CGPoint(x: 150, y: markerY))
                    if mode == "red" {
                        try await click(CGPoint(x: 300 - 6 - 24, y: geometry.frames["gradient-position:root"]!.maxY + 6 + 14))
                    }
                    let (insertionField, insertionDelegate) = try await draft(identifier, "37 %")
                    // The inserted quarter stop takes index 1 from the selected
                    // middle stop. Index equality must not preserve its draft.
                    try await click(CGPoint(x: 81, y: markerY))
                    let inserted = store.state["layer_properties"]["controls"].stableKey
                    let stops = store.state["layer_properties"]["controls"].array.first { $0["key"].string == key }!["value"]["value"].array
                    try require(stops.count == 4 && abs(stops[1]["position"].number - 0.25) < 0.0001,
                        "The native plot must insert a new stop at the old selected index")
                    try require(geometry.frames[identifier + ":entry"] == nil,
                        "Insertion must discard the old field's visible draft")
                    try await commit(insertionField, insertionDelegate)
                    try require(store.state["layer_properties"]["controls"].stableKey == inserted,
                        "A previous stop's unfinished field must not edit a new stop reusing its index")
                    note("PASS: platform \(platform), gradient insertion retires the \(mode) field even when its index is reused")

                    if mode == "red" {
                        try await click(CGPoint(x: 300 - 6 - 24, y: geometry.frames["gradient-position:root"]!.maxY + 6 + 14))
                    }
                    let (historyField, historyDelegate) = try await draft(identifier, "37 %")
                    try await action(["type": "invoke", "command": "undo"])
                    try require(store.state["layer_properties"]["controls"].stableKey == accepted,
                        "One Undo must remove the inserted stop and restore the original gradient")
                    try await commit(historyField, historyDelegate)
                    try require(store.state["layer_properties"]["controls"].stableKey == accepted,
                        "A field from the undone stop must not edit its replacement")
                    try await action(["type": "invoke", "command": "redo"])
                    try require(store.state["layer_properties"]["controls"].stableKey == inserted,
                        "An ignored callback must preserve insertion Redo")
                    note("PASS: platform \(platform), gradient Undo/Redo retires the \(mode) draft and retains history")

                    if mode == "opacity" {
                        let (removedField, removedDelegate) = try await draft(identifier, "37 %")
                        let footerY = geometry.frames["gradient-opacity:root"]!.maxY + 6 + 10
                        try await click(CGPoint(x: 45, y: footerY))
                        try require(store.state["layer_properties"]["controls"].stableKey == accepted,
                            "Remove stop must remove the selected insertion")
                        try await commit(removedField, removedDelegate)
                        try require(store.state["layer_properties"]["controls"].stableKey == accepted,
                            "A removed stop's field must not change the remaining gradient")
                        for iteration in 0..<2 {
                            let (resetField, resetDelegate) = try await draft(identifier, "37 %")
                            try await click(CGPoint(x: 260, y: footerY))
                            try require(store.state["layer_properties"]["controls"].stableKey == defaultControls,
                                "The native Reset button must restore the shared gradient defaults")
                            try await commit(resetField, resetDelegate)
                            try require(store.state["layer_properties"]["controls"].stableKey == defaultControls,
                                "Reset must discard a draft even when the stop count is unchanged (\(iteration))")
                        }
                        note("PASS: platform \(platform), gradient Remove and repeated Reset discard unfinished opacity drafts")
                    }
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
