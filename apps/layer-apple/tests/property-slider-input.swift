import AppKit
import SwiftUI

@MainActor private final class PropertySliderGeometry { var frames: [String: CGRect] = [:] }

/// Native slider contacts through the real property views and serial owner.
/// Temporary windows and in-memory documents; no renderer or artist storage.
@main final class PropertySliderChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        for platform: UInt32 in [0, 1] {
            for (effect, mode) in [("", "opacity"), ("paper", "opacity"), ("", "layer-opacity"),
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
                let key = effect == "gradient_map" ? "gradient" : effect == "split_tone" ? "shadows"
                    : effect == "brightness_contrast" ? "brightness" : "opacity"
                if effect == "gradient_map" {
                    try await action(["type": "effect", "action": ["op": "gradient_stop", "layer": layer,
                        "key": key, "index": NSNull(), "position": 0.5, "remove": false]])
                }
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
                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3)))
                let host = NSHostingView(rootView: content)
                window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                try await drain(0.3)
                func click(_ point: CGPoint) async throws {
                    for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                        try event(type, at: point, marker: host, number: 1); try await drain()
                    }
                }
                if effect == "gradient_map" {
                    let position = geometry.frames["gradient-position:root"]!
                    // The middle stop is 43 points down the 52-point ramp above Position.
                    try await click(CGPoint(x: 150, y: position.minY - 6 - 52 + 43))
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
