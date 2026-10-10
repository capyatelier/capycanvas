import SwiftUI

struct PressureCalibration: View {
    @ObservedObject var store: EditorStore
    @GestureState private var touching = false
    @State private var dragging = false
    @State private var bodySize = CGSize(width: 336, height: 324)
    private var view: JSON { store.state["pressure_calibration"] }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func send(_ action: [String: Any]) { store.dispatch(["type": "pressure_calibration", "action": action]) }
    private func reset() { store.dispatch(["type": "curve_editor", "target": ["kind": "pressure"], "action": ["kind": "reset"]]) }
    private func escape() {
        store.dispatch(["type": "curve_editor", "target": ["kind": "pressure"], "action": ["kind": "key",
            "epoch": view["editor"]["controls"]["epoch"].uint, "key_event": "Escape", "pressed": true, "repeat": false,
            "modifiers": ["command": false, "shift": false, "alt": false]]])
    }
    var body: some View {
        if !view.isNull {
            GeometryReader { viewport in
                let bounds = view["bounds"], width = min(bounds["width"].number, viewport.size.width)
                VStack(spacing: 0) {
                    HStack(spacing: 8) {
                        Text(view["title"].string).fontWeight(.bold).frame(maxWidth: .infinity, minHeight: 28, alignment: .leading)
                            .contentShape(Rectangle())
                            .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("editor-workspace"))
                                .updating($touching) { _, active, _ in active = true }
                                .onChanged { event in
                                    if !dragging {
                                        dragging = true
                                        send(["kind": "drag", "phase": "down", "position": [event.startLocation.x, event.startLocation.y],
                                            "viewport": [viewport.size.width, viewport.size.height]])
                                    }
                                    send(["kind": "drag", "phase": "move", "position": [event.location.x, event.location.y],
                                        "viewport": [viewport.size.width, viewport.size.height]])
                                }.onEnded { event in
                                    send(["kind": "drag", "phase": "up", "position": [event.location.x, event.location.y],
                                        "viewport": [viewport.size.width, viewport.size.height]])
                                    dragging = false
                                })
                        IconTile(icon: "window-close", label: view["close"].string) { send(["kind": "cancel"]) }
                            .frame(width: 24, height: 24).accessibilityIdentifier("pen-pressure-close")
                    }.padding(.leading, 12).padding(.trailing, 6).padding(.vertical, 4).background(palette["tabbar"])
                    EditorScrollView(.vertical) {
                        VStack(spacing: 10) {
                            CurveEditor(store: store, control: view["editor"], target: JSON(["kind": "pressure"]), label: view["title"].string)
                            HStack(spacing: 8) {
                                Button(view["firmer"].string) { send(["kind": "sensitivity", "lighter": false]) }
                                    .disabled(!view["firmer_enabled"].bool).accessibilityIdentifier("pen-pressure-firmer")
                                Button(view["lighter"].string) { send(["kind": "sensitivity", "lighter": true]) }
                                    .disabled(!view["lighter_enabled"].bool).accessibilityIdentifier("pen-pressure-lighter")
                            }.frame(maxWidth: .infinity)
                            HStack(spacing: 8) {
                                Button(view["reset"].string, action: reset).accessibilityIdentifier("pen-pressure-reset")
                                Spacer(minLength: 0)
                                Button(view["cancel"].string) { send(["kind": "cancel"]) }.accessibilityIdentifier("pen-pressure-cancel")
                                Button(view["apply"].string) { send(["kind": "apply"]) }
                                    .buttonStyle(.borderedProminent).accessibilityIdentifier("pen-pressure-apply")
                            }
                        }.padding(12).fixedSize(horizontal: false, vertical: true)
                            .onGeometryChange(for: CGSize.self) { $0.size } action: { bodySize = $0 }
                    }.frame(height: min(bodySize.height, max(0, viewport.size.height - 36)))
                }.frame(width: width).background(palette["panel"]).clipShape(SquircleShape.surface.fittedClip)
                    .background { OutsideShadow(shape: SquircleShape.surface, opacity: 0.16, radius: 4, y: 2) }
                    .onGeometryChange(for: CGSize.self) { $0.size } action: { size in
                        send(["kind": "measure", "extent": [size.width, size.height], "viewport": [viewport.size.width, viewport.size.height]])
                    }
                    .onChange(of: viewport.size) { _, size in
                        send(["kind": "measure", "extent": [width, min(bodySize.height + 36, size.height)], "viewport": [size.width, size.height]])
                    }
                    .onChange(of: touching) { _, active in
                        if !active && dragging { dragging = false; send(["kind": "drag", "phase": "cancel", "position": [0, 0], "viewport": [viewport.size.width, viewport.size.height]]) }
                    }
                    .onKeyPress(.escape) { guard !NativeTextContext.composing else { return .ignored }; escape(); return .handled }
                    .offset(x: bounds["x"].number, y: bounds["y"].number)
                    .accessibilityIdentifier("pen-pressure-dialog")
            }.zIndex(1100)
        }
    }
}
