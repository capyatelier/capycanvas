import SwiftUI

struct PenResponseCalibration: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        let kind = store.state["tilt_calibration"].isNull ? "pressure" : "tilt"
        if !store.state[kind + "_calibration"].isNull {
            PenResponsePanel(store: store, kind: kind).id(kind).zIndex(1100)
        }
    }
}

private struct PenResponsePanel: View {
    @ObservedObject var store: EditorStore
    let kind: String
    @GestureState private var touching = false
    @State private var dragging = false
    @State private var bodySize = CGSize(width: 352, height: 440)
    @State private var headerHeight: CGFloat = 72
    @State private var footerHeight: CGFloat = 68
    @State private var measuredSize: CGSize?
    @State private var measuredViewport = CGSize.zero
    @State private var measurementAccepted = false
    @State private var exposed = false
    private var tilt: Bool { kind == "tilt" }
    private var prefix: String { "pen-" + kind }
    private var view: JSON { store.state[kind + "_calibration"] }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var fitted: Bool {
        guard measurementAccepted, let measuredSize else { return false }
        return abs(view["bounds"]["width"].number - measuredSize.width) < 0.5 && abs(view["bounds"]["height"].number - measuredSize.height) < 0.5
    }
    private func panelAction(_ action: [String: Any]) -> [String: Any] {
        tilt ? ["type": "tilt_calibration", "action": ["kind": "panel", "action": action]] : ["type": "pressure_calibration", "action": action]
    }
    private func send(_ action: [String: Any]) { store.dispatch(panelAction(action)) }
    private func measure(_ size: CGSize, viewport: CGSize) {
        measuredSize = size; measuredViewport = viewport; measurementAccepted = false
        store.edit(panelAction(["kind": "measure", "extent": [size.width, size.height], "viewport": [viewport.width, viewport.height]])) { error in
            if measuredSize == size && measuredViewport == viewport { measurementAccepted = error == nil }
        }
    }
    private func curve(_ action: [String: Any]) { store.dispatch(["type": "curve_editor", "target": ["kind": kind], "action": action]) }
    private func escape() {
        curve(["kind": "key", "epoch": view["editor"]["controls"]["epoch"].uint,
            "key_event": "Escape", "pressed": true, "repeat": false, "modifiers": ["command": false, "shift": false, "alt": false]])
    }
    var body: some View {
        GeometryReader { viewport in
            let bounds = view["bounds"], width = min(bounds["width"].number, viewport.size.width)
            VStack(spacing: 0) {
                HStack(alignment: .top, spacing: 8) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(view["title"].string).font(.headline).frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
                        if tilt { Text(view["device"].string).font(.caption).foregroundStyle(palette["text"].opacity(0.7)) }
                    }.contentShape(Rectangle())
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
                        .frame(width: 24, height: 24).accessibilityIdentifier(prefix + "-close")
                }.padding(16)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { headerHeight = $0 }
                EditorScrollView(.vertical) {
                    VStack(spacing: 12) {
                        CurveEditor(store: store, control: view["editor"], target: JSON(["kind": kind]), label: view["title"].string,
                            axisGutter: view["axis_gutter"].string, plotOverlay: tilt ? AnyView(directionCircle.padding(8)) : nil).id(kind)
                        HStack(spacing: 8) {
                            if tilt { Text(view["sensitivity"].string).frame(maxWidth: .infinity, alignment: .leading) }
                            HStack(spacing: tilt ? 0 : 8) {
                                sensitivity("firmer", lighter: false, icon: "minus")
                                sensitivity("lighter", lighter: true, icon: "plus")
                            }
                            if tilt { Text(view["direction"].string).frame(maxWidth: .infinity, alignment: .leading); direction }
                        }
                    }.padding(.horizontal, 16).fixedSize(horizontal: false, vertical: true)
                        .onGeometryChange(for: CGSize.self) { $0.size } action: { bodySize = $0 }
                }.frame(height: min(bodySize.height, max(0, viewport.size.height - headerHeight - footerHeight)))
                HStack(spacing: 8) {
                    Button(view["reset"].string) { curve(["kind": "reset"]) }.accessibilityIdentifier(prefix + "-reset")
                    Spacer(minLength: 0)
                    Button(view["cancel"].string) { send(["kind": "cancel"]) }.accessibilityIdentifier(prefix + "-cancel")
                    Button(view["apply"].string) { send(["kind": "apply"]) }.buttonStyle(.borderedProminent)
                        .disabled(tilt && !view["ready"].bool).accessibilityIdentifier(prefix + "-apply")
                }.controlSize(.large).frame(minHeight: 36).padding(16)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { footerHeight = $0 }
            }.frame(width: width).background(palette["panel"]).clipShape(SquircleShape.surface.fittedClip)
                .background { OutsideShadow(shape: SquircleShape.surface, opacity: 0.16, radius: 4, y: 2) }
                .onGeometryChange(for: CGSize.self) { $0.size } action: { size in
                    measure(size, viewport: viewport.size)
                }
                .onChange(of: viewport.size) { _, size in
                    measure(CGSize(width: min(bounds["width"].number, size.width), height: min(bodySize.height + headerHeight + footerHeight, size.height)), viewport: size)
                }
                .onChange(of: touching) { _, active in
                    if !active && dragging { dragging = false; send(["kind": "drag", "phase": "cancel", "position": [0, 0], "viewport": [viewport.size.width, viewport.size.height]]) }
                }
                .onKeyPress(.escape) { guard !NativeTextContext.composing else { return .ignored }; escape(); return .handled }
                .offset(x: bounds["x"].number, y: bounds["y"].number)
                .opacity(exposed ? 1 : 0).allowsHitTesting(exposed).accessibilityHidden(!exposed)
                .onChange(of: fitted, initial: true) { _, ready in if ready { exposed = true; store.measuredPenResponse = kind } }
                .onDisappear { if store.measuredPenResponse == kind { store.measuredPenResponse = nil } }
                .accessibilityElement(children: .contain).accessibilityIdentifier(prefix + "-dialog")
        }
    }
    @ViewBuilder private func sensitivity(_ key: String, lighter: Bool, icon: String) -> some View {
        let control = Button { send(["kind": "sensitivity", "lighter": lighter]) } label: {
            HStack(spacing: 6) { SharedIcon(name: icon); if !tilt { Text(view[key].string) } }
                .frame(maxWidth: tilt ? nil : .infinity, minHeight: 36)
                .frame(width: tilt ? 36 : nil)
                .background(tilt ? palette["input"] : .clear, in: SquircleShape.control.segment(lighter ? 1 : 0, of: 2))
        }.disabled(!view[key + "_enabled"].bool).accessibilityIdentifier(prefix + "-" + key)
            .accessibilityLabel(view[key].string).help(view[key].string)
        if tilt { control.buttonStyle(.plain) } else { control.buttonStyle(.bordered) }
    }
    private var directionCircle: some View {
        Canvas { context, size in
            let center = CGPoint(x: size.width / 2, y: size.height / 2), radius = min(size.width, size.height) / 2 - 5
            context.stroke(Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2)),
                with: .color(palette["text"].opacity(0.18)), lineWidth: 1)
            for key in ["input_direction", "adjusted_direction"] {
                let vector = view[key]
                if !vector.isNull {
                    let x = vector[0].number * radius, y = vector[1].number * radius, length = hypot(x, y)
                    var arrow = Path(); arrow.move(to: center); arrow.addLine(to: CGPoint(x: center.x + x, y: center.y + y))
                    if length > 3 {
                        let ux = x / length, uy = y / length
                        arrow.move(to: CGPoint(x: center.x + x - ux * 6 - uy * 3, y: center.y + y - uy * 6 + ux * 3))
                        arrow.addLine(to: CGPoint(x: center.x + x, y: center.y + y))
                        arrow.addLine(to: CGPoint(x: center.x + x - ux * 6 + uy * 3, y: center.y + y - uy * 6 - ux * 3))
                    }
                    let adjusted = key == "adjusted_direction"
                    context.stroke(arrow, with: .color(adjusted ? palette.accent : palette["text"].opacity(0.55)), lineWidth: adjusted ? 2.5 : 1.5)
                }
            }
        }.frame(width: 72, height: 72).accessibilityLabel(view["direction"].string).accessibilityIdentifier("pen-tilt-direction")
            .contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).onChanged { _ in })
    }
    private var direction: some View {
        HStack(spacing: 0) {
            axis("reverse_horizontal", label: "reverse_horizontal", icon: "flip-horizontal", index: 0)
            axis("reverse_vertical", label: "reverse_vertical", icon: "flip-vertical", index: 1)
            axis("swap", label: "swap_axes", icon: "swap-orientation", index: 2)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("pen-tilt-axes")
    }
    private func axis(_ key: String, label: String, icon: String, index: Int) -> some View {
        let active = view["axes"][key].bool
        return Button {
            var axes = view["axes"].object; axes[key] = !active
            store.dispatch(["type": "tilt_calibration", "action": ["kind": "axes", "axes": axes]])
        } label: {
            SharedIcon(name: icon).frame(width: 36, height: 36)
                .background(active ? palette.active : palette["input"], in: SquircleShape.control.segment(index, of: 3))
                .contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(view[label].string).help(view[label].string)
            .accessibilityValue(active ? "1" : "0").accessibilityAddTraits(active ? .isSelected : [])
            .accessibilityIdentifier("pen-tilt-" + (key == "swap" ? "swap-axes" : key.replacingOccurrences(of: "_", with: "-")))
    }
}
