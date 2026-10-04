import SwiftUI

struct GradientEditor: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    @ObservedObject var store: EditorStore
    let control: JSON
    var identifier = "gradient"
    let send: ToolOptionEdit
    @Environment(\.isEnabled) private var enabled
    @State private var selected = 0
    @State private var owner = ""
    @State private var revision: UInt64 = 0
    @State private var contact: (target: JSON, index: Int, x: CGFloat, position: Double)?
    @State private var held: (target: JSON, index: Int)?
    @FocusState private var focused: Bool
    @GestureState private var touching = false
    private var gradient: JSON { control["gradient"] }
    private var target: JSON { gradient["destination"] }
    private var stops: [JSON] { control["value"]["value"]["stops"].array }
    private var index: Int { max(0, min(selected, stops.count - 1)) }
    private var interior: Bool { index > 0 && index < stops.count - 1 }
    private var field: String { "\(owner):\(revision):\(index):\(stops.count)" }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var ownerKey: String {
        JSON([store.state["document_file"]["epoch"].raw, target["kind"].raw, target["layer"].raw, target["key"].raw]).stableKey
    }
    private func edit(_ edit: [String: Any], phase: String? = nil, target: JSON? = nil,
        completion: @escaping @MainActor (String?) -> Void = { _ in }) {
        let action: [String: Any] = ["op": "gradient", "target": (target ?? self.target).raw, "edit": edit]
        send(["type": "effect", "action": phase.map { ["op": "gesture", "phase": $0, "action": action] } ?? action], completion)
    }
    private func stop(_ position: Double, index: Int?) -> [String: Any] {
        ["kind": "stop", "index": index as Any? ?? NSNull(), "position": position, "color": NSNull(), "remove": false]
    }
    private func cancel() {
        guard let owner = contact?.target ?? held?.target else { return }
        contact = nil; held = nil
        edit(["kind": "reset"], phase: "cancel", target: owner)
    }
    private func remove() {
        let removed = index
        selected = max(0, removed - 1)
        edit(["kind": "stop", "index": removed, "position": 0, "color": NSNull(), "remove": true])
    }
    private func step(_ steps: Int) {
        let phase = held == nil ? "down" : "move"
        if held == nil { held = (target, index) }
        guard let held else { return }
        edit(["kind": "position", "index": held.index, "operation": ["type": "step", "steps": steps]], phase: phase, target: held.target)
    }
    private func release() {
        guard let held else { return }
        self.held = nil
        edit(["kind": "position", "index": held.index, "operation": ["type": "step", "steps": 0]], phase: "up", target: held.target)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                let modes = gradient["interpolations"].array
                EditorChoice(label: gradient["interpolation_label"].string, options: modes.map { $0[1].string },
                    selected: modes.firstIndex { $0[0].string == control["value"]["value"]["interpolation"].string } ?? 0,
                    identifier: identifier + "-interpolation", background: palette["input"]) {
                    edit(["kind": "interpolation", "value": modes[$0][0].raw])
                }.frame(maxWidth: .infinity)
                action("flip-horizontal", gradient["reverse_label"].string, "reverse") {
                    selected = stops.count - 1 - index; revision &+= 1
                    edit(["kind": "reverse"])
                }
                action("reset", nativeCopy["color"]["reset_gradient"].string, "reset") { revision &+= 1; edit(["kind": "reset"]) }
            }
            strip
            HStack(spacing: 6) {
                NumberControl(store: store, label: nativeCopy["color"]["position"].string, value: stops.isEmpty ? 0 : stops[index]["position"].number,
                    control: store.catalog["opacity"], identifier: identifier + "-position", valueOnly: true,
                    gestureChange: { [index, target, field] phase, value, completion in
                        guard phase == "cancel" || field == self.field else { completion(nil); return }
                        edit(["kind": "position", "index": index, "operation": ["type": "value", "value": value]], phase: phase, target: target, completion: completion)
                    }) { [index, field] value, completion in
                    guard field == self.field else { completion(nil); return }
                    edit(["kind": "position", "index": index, "operation": ["type": "value", "value": value]], completion: completion)
                }.disabled(!interior).frame(maxWidth: .infinity).id(field)
                action("minus", nativeCopy["color"]["remove_stop"].string, "remove", enabled: interior, run: remove)
                if !stops.isEmpty {
                    ManagedColorButton(label: nativeCopy["color"]["color"].string, identifier: identifier + "-stop", value: stops[index]["color"],
                        documentSpace: store.state["colors"]["rgb_space"].string, viewing: store.colorViewing, titled: false, swatchWidth: 36) { [index] color in
                        edit(["kind": "stop", "index": index, "position": stops[index]["position"].number, "color": color.raw, "remove": false])
                    }.fixedSize()
                }
                action("fill", nativeCopy["color"]["use_selected"].string, "use-color") { edit(["kind": "use_current_color", "index": index]) }
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier(identifier)
            .onChange(of: ownerKey, initial: true) { _, next in
                if owner != next { cancel(); selected = 0; owner = next }
            }
            .onDisappear(perform: cancel)
    }
    private func action(_ icon: String, _ label: String, _ name: String, enabled: Bool = true, run: @escaping () -> Void) -> some View {
        IconTile(icon: icon, label: label, enabled: enabled, action: run).frame(width: 32, height: 28)
            .accessibilityIdentifier("\(identifier)-\(name)").modifier(NumberControlMeasurement(id: "\(identifier)-\(name):root"))
    }
    private var strip: some View {
        GeometryReader { geometry in
            let width = max(1, geometry.size.width - 12)
            ZStack(alignment: .topLeading) {
                GradientPreview(store: store, gradient: control["value"]["value"])
                    .frame(width: width, height: 32).clipShape(RoundedRectangle(cornerRadius: 4)).offset(x: 6)
                ForEach(stops.indices, id: \.self) { stop in
                    Circle().fill(ColorUI.preview(stops[stop]["color"])["rgba"].paintColor)
                        .overlay(Circle().strokeBorder(palette["text"], lineWidth: 1))
                        .frame(width: 10, height: 10)
                        .padding(1)
                        .overlay(Circle().strokeBorder(palette["text"], lineWidth: 2).padding(-2).opacity(stop == index ? 1 : 0))
                        .accessibilityElement().accessibilityLabel("\(nativeCopy["color"]["color"].string) \(stop + 1)")
                        .accessibilityAddTraits(stop == index ? .isSelected : []).accessibilityIdentifier("\(identifier)-marker-\(stop)")
                        .position(x: 6 + stops[stop]["position"].number * width, y: 32 + 7)
                }
            }.frame(maxWidth: .infinity, alignment: .topLeading).contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).updating($touching) { _, active, _ in active = true }.onChanged { event in
                    if contact == nil {
                        guard enabled else { return }
                        cancel()
                        let position = max(0, min(1, (event.startLocation.x - 6) / width))
                        let found = stops.indices.first { abs(stops[$0]["position"].number - position) * width < 8 }
                        guard found != nil || gradient["can_add"].bool else { return }
                        let at = found ?? stops.filter { $0["position"].number < position }.count
                        selected = at; focused = true
                        let start = found.map { stops[$0]["position"].number } ?? position
                        contact = (target, at, event.startLocation.x, start)
                        edit(stop(start, index: found), phase: "down", target: target)
                    } else if let c = contact {
                        edit(stop(max(0, min(1, c.position + (event.location.x - c.x) / width)), index: c.index), phase: "move", target: c.target)
                    }
                }.onEnded { event in
                    guard let c = contact else { return }
                    contact = nil
                    edit(stop(max(0, min(1, c.position + (event.location.x - c.x) / width)), index: c.index), phase: "move", target: c.target)
                    edit(["kind": "position", "index": c.index, "operation": ["type": "step", "steps": 0]], phase: "up", target: c.target)
                })
                .onChange(of: touching) { _, active in if !active && contact != nil { cancel() } }
        }.frame(height: 46).modifier(NumberControlMeasurement(id: identifier + "-strip:track"))
            .focusable(enabled).focused($focused).focusEffectDisabled()
            .onKeyPress(keys: [.leftArrow, .rightArrow], phases: [.down, .repeat, .up]) { press in
                guard enabled else { return .ignored }
                if press.phase == .up { release(); return .handled }
                step((press.key == .leftArrow ? -1 : 1) * (press.modifiers.contains(.shift) ? 10 : 1))
                return .handled
            }
            .onKeyPress(keys: [.delete, .deleteForward]) { _ in
                guard enabled else { return .ignored }
                cancel(); if interior { remove() }
                return .handled
            }
            .onKeyPress(.escape) {
                guard contact != nil || held != nil else { return .ignored }
                cancel(); return .handled
            }
            .onChange(of: focused) { _, focused in if !focused { cancel() } }
            .accessibilityElement(children: .contain).accessibilityLabel(nativeCopy["color"]["add_stop"].string)
            .accessibilityIdentifier(identifier + "-strip")
            .help(nativeCopy["color"]["add_stop"].string)
    }
}

struct GradientPreview: View {
    @ObservedObject var store: EditorStore
    let gradient: JSON
    @Environment(\.displayScale) private var scale
    @State private var image: CGImage?
    var body: some View {
        GeometryReader { geometry in
            let size = [min(2048, max(1, Int((geometry.size.width * scale).rounded()))), min(64, max(1, Int((geometry.size.height * scale).rounded())))]
            Canvas { graphics, area in
                graphics.fillTransparencyChecker(area, palette: EditorPalette(source: store.state["palette"]))
                if let image { graphics.draw(Image(decorative: image, scale: 1), in: CGRect(origin: .zero, size: area)) }
            }.task(id: JSON([gradient.raw, store.state["colors"]["rgb_space"].raw, store.colorViewing["document_depth"].raw,
                store.colorViewing["recipe"].raw, size]).stableKey) {
                let request = JSON(["type": "gradient", "gradient": gradient.raw, "document_space": store.state["colors"]["rgb_space"].raw,
                    "display_space": "DisplayP3", "rendition": store.colorViewing["recipe"].raw,
                    "image": ["size": size, "depth": store.colorViewing["document_depth"].raw]])
                let rendered = await Task.detached(priority: .userInitiated) { GradientPreview.render(ColorUI.resolve(request.object)) }.value
                if !Task.isCancelled { image = rendered }
            }
        }
    }
    nonisolated static func render(_ result: JSON) -> CGImage? {
        let width = Int(result["size"][0].uint), height = Int(result["size"][1].uint), argb = result["argb"].array
        guard width > 0, height > 0, argb.count == width * height,
            let space = CGColorSpace(name: CGColorSpace.displayP3) else { return nil }
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        for (i, pixel) in argb.enumerated() {
            let v = UInt32(pixel.uint), alpha = v >> 24
            func premultiplied(_ c: UInt32) -> UInt8 { UInt8((c * alpha + 127) / 255) }
            bytes[i * 4] = premultiplied(v >> 16 & 255); bytes[i * 4 + 1] = premultiplied(v >> 8 & 255)
            bytes[i * 4 + 2] = premultiplied(v & 255); bytes[i * 4 + 3] = UInt8(alpha)
        }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue), provider: provider,
            decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }
}
