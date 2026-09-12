// Actual SwiftUI body invalidation and rendered-value propagation. No XCTest,
// visible editor, GPU, native menu automation or artist persistence is involved.
import AppKit
import SwiftUI

@MainActor private final class ViewProbe {
    var bodies: [String: Int] = [:]
    var rendered: [String: String] = [:]
}
private struct RenderedValue: NSViewRepresentable {
    let name: String, value: String
    let probe: ViewProbe
    func makeNSView(context: Context) -> NSView { NSView() }
    func updateNSView(_ view: NSView, context: Context) { probe.rendered[name] = value }
}
private struct Readout: View {
    let name: String, probe: ViewProbe
    let read: () -> String
    var body: some View {
        probe.bodies[name, default: 0] += 1
        return RenderedValue(name: name, value: read(), probe: probe).frame(width: 40, height: 30)
    }
}
private struct EditorReadouts: View {
    let model: EditorSnapshotState, probe: ViewProbe
    var body: some View {
        if model.state.isNull { RenderedValue(name: "loading", value: "yes", probe: probe) }
        else {
            HStack {
                Readout(name: "undo", probe: probe) { String(model.command("undo")["enabled"].bool) }
                Readout(name: "color", probe: probe) { model.panel("color")["label"].string }
                Readout(name: "menu", probe: probe) { String(model.applicationMenu("edit")["enabled"].bool) }
                Readout(name: "layout", probe: probe) { String(model.snapshot["layout"]["width"].number) }
                // The parent only passes a reader. A child invalidated later
                // must fetch current data without needing its parent to rebuild.
                Readout(name: "camera", probe: probe) { [fields = model.state] in String(fields["camera"]["zoom"].number) }
            }
        }
    }
}

@main struct SnapshotViewChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let model = EditorSnapshotState(), probe = ViewProbe()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 320, height: 100),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: EditorReadouts(model: model, probe: probe))
        defer { window.contentView = nil; window.close() }
        func settle() async throws {
            for _ in 0..<8 {
                window.contentView?.layoutSubtreeIfNeeded()
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func wait(_ name: String, _ expected: String) async throws {
            let deadline = Date().addingTimeInterval(5)
            while probe.rendered[name] != expected {
                guard Date() < deadline else { throw NSError(domain: "Missing rendered \(name): \(probe.rendered)", code: 1) }
                try await settle()
            }
            try await settle()
        }
        func snapshot(_ enabled: Bool, color: String = "Color") -> JSON {
            JSON(["state": ["commands": [["id": "undo", "enabled": enabled]], "camera": ["zoom": 1]],
                  "panels": [["id": "color", "label": color]],
                  "application_menus": [["id": "edit", "enabled": enabled]], "layout": ["width": 1200]])
        }
        try await wait("loading", "yes")
        model.receive(snapshot(false))
        try await wait("undo", "false")
        precondition(probe.rendered["color"] == "Color" && probe.rendered["menu"] == "false")
        let initial = probe.bodies
        model.receive(snapshot(true))
        try await wait("undo", "true"); try await wait("menu", "true")
        precondition(probe.bodies["undo"]! > initial["undo"]! && probe.bodies["menu"]! > initial["menu"]!)
        precondition(probe.bodies["color"] == initial["color"] && probe.bodies["layout"] == initial["layout"]
                     && probe.bodies["camera"] == initial["camera"], "Command updates must not rebuild unrelated bodies")
        let beforeCamera = probe.bodies
        model.receive(JSON(["camera": ["zoom": 2], "revision": 3]))
        try await wait("camera", "2.0")
        for name in ["undo", "color", "menu", "layout"] { precondition(probe.bodies[name] == beforeCamera[name]) }
        let beforeNoOp = probe.bodies
        model.receive(try JSON.decode(model.snapshot.json.encoded()))
        try await settle()
        precondition(probe.bodies == beforeNoOp, "An equivalent full snapshot must not rebuild any body")
        model.receive(snapshot(true, color: "Colour"))
        try await wait("color", "Colour")
        print("SwiftUI snapshot views passed: initial publication, command/menu changes, unchanged bodies, camera patches and no-op snapshots")
    }
}
