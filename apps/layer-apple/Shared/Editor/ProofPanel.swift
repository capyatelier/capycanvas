import SwiftUI
import UniformTypeIdentifiers

struct ProofPanel: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var controller: ProofController
    private var model: JSON { store.snapshot["proof_panel"] }
    var body: some View {
        VStack(spacing: 6) {
            HStack(spacing: 2) {
                ForEach(["off", "sdr", "print"], id: \.self) { mode in
                    Button {
                        controller.close(); controller.action(["type": "mode", "mode": mode])
                    } label: {
                        Text(mode == "sdr" ? "SDR" : mode.capitalized).frame(maxWidth: .infinity).padding(.vertical, 5)
                            .background(model["mode"].string == mode ? Color.primary.opacity(0.12) : .clear,
                                in: SquircleShape.control)
                    }.buttonStyle(.plain).disabled(mode == "sdr" && !model["hdr"].bool)
                        .accessibilityIdentifier("proof-mode-" + mode)
                        .accessibilityAddTraits(model["mode"].string == mode ? .isSelected : [])
                }
            }.accessibilityElement(children: .contain)
            if model["mode"].string == "sdr" {
                ProofDial(store: store, controller: controller)
            } else if model["mode"].string == "print" {
                EditorScrollView { PrintProofControls(controller: controller, model: model, store: store) }
            } else { Spacer(minLength: 0) }
        }.padding(.horizontal, 8).padding(.vertical, 6)
            .modifier(PanelBodyMeasurement(panel: "proof", kind: .fixed))
    }
}

/// One shared immutable 512² guide, generated away from UI and render owners.
@MainActor final class ProofGlass: ObservableObject {
    static let shared = ProofGlass()
    @Published private(set) var image: CGImage?
    private init() {
        DispatchQueue.global(qos: .userInitiated).async {
            var bytes = Data(count: 512 * 512 * 4)
            guard bytes.withUnsafeMutableBytes({ capy_apple_proof_texture(512, $0.bindMemory(to: UInt8.self).baseAddress, $0.count) }),
                let provider = CGDataProvider(data: bytes as CFData), let space = CGColorSpace(name: CGColorSpace.sRGB),
                let image = CGImage(width: 512, height: 512, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 2048,
                    space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider,
                    decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else { return }
            DispatchQueue.main.async { self.image = image }
        }
    }
}

struct ProofDial: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var controller: ProofController
    @ObservedObject private var glass = ProofGlass.shared
    @State private var numeric = false
    @State private var origin = CGPoint.zero
    private var model: JSON { store.snapshot["proof_panel"] }
    var body: some View {
        GeometryReader { bounds in
            let glassImage = glass.image
            let size = max(128, min(bounds.size.width, bounds.size.height))
            let g = ColorUI.resolve(["type": "proof_dial", "size": size, "recipe": model["recipe"].raw])
            let markers = [g["marker"], g["arcs"][0]["point"], g["arcs"][1]["point"]]
            ZStack(alignment: .topLeading) {
                Canvas { graphics, _ in
                    let radius = g["radius"].number
                    let center = point(g["center"])
                    let rect = CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2)
                    var clipped = graphics; clipped.clip(to: Path(ellipseIn: rect))
                    if let image = glassImage { clipped.draw(Image(decorative: image, scale: 1), in: rect) }
                    else { clipped.fill(Path(rect), with: .color(.gray)) }
                    for i in 0..<2 {
                        let arc = g["arcs"][i], points = arc["path"].array
                        let colors: [Color] = i == 0 ? [Color(white: 0.04), Color(white: 0.55), .white]
                            : [Color(white: 0.95), Color(red: 0.15, green: 0.55, blue: 0.85)]
                        graphics.stroke(path(points), with: .linearGradient(Gradient(colors: colors),
                            startPoint: point(points.first ?? JSON()), endPoint: point(points.last ?? JSON())),
                            style: StrokeStyle(lineWidth: arc["geometry"]["width"].number, lineCap: .round))
                    }
                    let markerRadius = g["arcs"][0]["geometry"]["marker_radius"].number
                    for marker in markers {
                        let p = point(marker), circle = Path(ellipseIn: CGRect(x: p.x - markerRadius, y: p.y - markerRadius, width: markerRadius * 2, height: markerRadius * 2))
                        graphics.stroke(circle, with: .color(.black.opacity(0.65)), lineWidth: 4)
                        graphics.stroke(circle, with: .color(.white), lineWidth: 2)
                    }
                    let font = g["text_size"].number, ink = EditorPalette(source: store.state["palette"])["text"].opacity(0.62)
                    for i in 0..<4 {
                        let readout = g["readouts"][i], text = model["readouts"][i].string
                        if readout["curve"].isNull {
                            EditorTextMetrics.draw(text, size: font, weight: .regular, in: graphics,
                                baseline: CGPoint(x: readout["text"][0].number - EditorTextMetrics.width(text, size: font, weight: .regular) / 2,
                                    y: readout["text"][1].number), color: ink)
                        } else { curved(text, readout: readout, center: center, font: font, ink: ink, graphics: graphics) }
                    }
                }.id(glassImage != nil).allowsHitTesting(false)
                ForEach(0..<4, id: \.self) { i in
                    let icon = g["readouts"][i]["icon"]
                    SharedIcon(name: model["icons"][i].string)
                        .opacity(0.62)
                        .frame(width: icon[2].number, height: icon[3].number).offset(x: icon[0].number, y: icon[1].number)
                        .allowsHitTesting(false).accessibilityHidden(true)
                }
                ParameterInput(hdr: false, identity: String(store.state["document_file"]["epoch"].uint), nudge: { phase, part, delta in
                    controller.action(["type": "control", "phase": phase, "part": Int(part) - 1,
                        "edit": ["type": "step", "axis": delta[0] != 0 ? 0 : 1, "steps": delta[0] + delta[1]]])
                }) { phase, part, point, size in
                    if phase == "reset" { controller.action(["type": "control", "part": Int(part) - 1, "edit": ["type": "reset"]]); return }
                    if phase == "down" { origin = point }
                    controller.action(["type": "dial", "phase": phase, "size": size, "origin": [origin.x, origin.y], "point": [point.x, point.y]])
                }.accessibilityHidden(true)
                Button { resetAll() } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.plain).frame(width: g["reset"][2].number, height: g["reset"][3].number)
                    .offset(x: g["reset"][0].number, y: g["reset"][1].number)
                    .accessibilityLabel("Reset SDR appearance").accessibilityIdentifier("proof-reset")
            }.frame(width: size, height: size)
                .contextMenu { Button("Edit SDR Appearance…") { numeric = true }; Button("Reset") { resetAll() } }
                .accessibilityElement(children: .contain)
                .accessibilityChildren {
                    ForEach(Array(["balance", "contrast", "exposure", "highlight_color"].enumerated()), id: \.offset) { i, key in
                        Text(["Fine texture balance", "Contrast", "Brightness", "Color intensity"][i])
                            .accessibilityValue(model["readouts"][[1, 0, 2, 3][i]].string)
                            .accessibilityAdjustableAction { direction in adjust(key, direction == .increment ? 1 : -1) }
                    }
                    Button("Reset SDR appearance") { resetAll() }
                        .accessibilityIdentifier("proof-reset")
                }
                .accessibilityIdentifier("proof-dial")
        }.aspectRatio(1, contentMode: .fit).frame(minWidth: 128, minHeight: 128)
            .sheet(isPresented: $numeric) { ProofNumbers(store: store, controller: controller).modifier(EditorPopupPresentation()) }
    }
    private func adjust(_ key: String, _ direction: Double) {
        let part = ["balance": 0, "contrast": 0, "exposure": 1, "highlight_color": 2][key] ?? 0
        controller.action(["type": "control", "part": part,
            "edit": ["type": "step", "axis": key == "contrast" ? 1 : 0, "steps": direction]])
    }
    private func resetAll() { controller.action(["type": "control", "part": 3, "edit": ["type": "reset"]]) }
    private func point(_ p: JSON) -> CGPoint { CGPoint(x: p[0].number, y: p[1].number) }
    private func path(_ points: [JSON]) -> Path {
        var p = Path(); for (i, v) in points.enumerated() { if i == 0 { p.move(to: point(v)) } else { p.addLine(to: point(v)) } }; return p
    }
    private func curved(_ text: String, readout: JSON, center: CGPoint, font: CGFloat, ink: Color, graphics: GraphicsContext) {
        let radius = readout["curve"][0].number, reverse = readout["curve"][2].bool
        let widths = text.map { EditorTextMetrics.width(String($0), size: font, weight: .regular) }
        var cursor = -widths.reduce(0, +) / 2
        for (i, c) in text.enumerated() {
            let angle = readout["curve"][1].number * .pi / 180 + (reverse ? -1 : 1) * (cursor + widths[i] / 2) / radius
            var local = graphics; local.translateBy(x: center.x + radius * cos(angle), y: center.y + radius * sin(angle))
            local.rotate(by: .radians(angle + (reverse ? -.pi / 2 : .pi / 2)))
            EditorTextMetrics.draw(String(c), size: font, weight: .regular, in: local, baseline: CGPoint(x: -widths[i] / 2, y: 0), color: ink)
            cursor += widths[i]
        }
    }
}

private struct ProofNumbers: View {
    @ObservedObject var store: EditorStore
    let controller: ProofController
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 12) {
            Text("SDR Appearance").font(.headline)
            ForEach(store.snapshot["proof_panel"]["numbers"].array, id: \.stableKey) { spec in
                let key = spec["key"].string
                NumberControl(store: store, label: spec["label"].string, value: spec["value"].number, control: spec["numeric"], gestureChange: { phase, value, completion in
                    controller.action(["type": "number", "phase": phase, "key": key, "value": value]); completion(nil)
                }) { value, completion in
                    controller.action(["type": "number", "key": key, "value": value]); completion(nil)
                }
            }
            Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
        }.padding(20).frame(minWidth: 300)
    }
}

private struct PrintProofControls: View {
    @ObservedObject var controller: ProofController
    let model: JSON
    @ObservedObject var store: EditorStore
    @State private var recipe = JSON()
    @State private var importing = false
    private var profiles: [JSON] { controller.form["profiles"].array }
    private func change(_ value: JSON) {
        let profile = JSON(["name": value["name"].raw, "profile": value["profile"].raw, "channels": "Rgb"])
        let normalized = ColorUI.resolve(["type": "print_proof", "settings": ["profile": profile.raw,
            "intent": value["conversion"]["intent"].raw, "bpc": value["conversion"]["black_point_compensation"].bool,
            "simulation": value["simulate_paper"].bool ? "paper_and_ink" : value["simulate_black_ink"].bool ? "black_ink" : "colors"]])
        guard normalized["error"].isNull else { controller.error = normalized["error"].string; return }
        recipe = normalized; controller.applyLive(normalized)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            PrintProfileMenu(controller: controller, recipe: recipe, busy: $importing) { p in
                change(recipe.replacing("name", with: p["name"]).replacing("profile", with: p["profile"]))
            }
            FormPicker("Simulate", selection: Binding(get: { recipe["simulate_paper"].bool ? "paper_and_ink" : recipe["simulate_black_ink"].bool ? "black_ink" : "colors" }, set: { v in
                change(recipe.replacing("simulate_paper", with: JSON(v == "paper_and_ink")).replacing("simulate_black_ink", with: JSON(v != "colors")))
            })) { ForEach(model["simulations"].array, id: \.stableKey) { Text($0["label"].string).tag($0["value"].string) } }
            FormPicker("Intent", selection: Binding(get: { recipe["conversion"]["intent"].string }, set: { v in
                change(recipe.replacing("conversion", with: recipe["conversion"].replacing("intent", with: JSON(v))))
            })) { ForEach(model["intents"].array, id: \.stableKey) { Text($0["label"].string).tag($0["value"].string) } }
            Toggle("Black point compensation", isOn: Binding(get: { recipe["conversion"]["black_point_compensation"].bool }, set: { v in
                change(recipe.replacing("conversion", with: recipe["conversion"].replacing("black_point_compensation", with: JSON(v))))
            })).disabled(recipe["conversion"]["intent"].string == "AbsoluteColorimetric")
            Toggle("Gamut warning", isOn: Binding(get: { model["gamut_warning"].bool }, set: { _ in store.invoke("gamut_warning") }))
                .disabled(model["print"].isNull).accessibilityIdentifier("proof-gamut-warning")
            if controller.busy { ProgressView("Preparing proof…") }
            if let error = controller.error { Text(error).foregroundStyle(.red).font(.caption) }
        }.disabled(controller.busy || importing)
            .task { controller.loadForm() }
            .onChange(of: controller.formRevision, initial: true) { _, _ in
                if !controller.form.isNull && !controller.busy { recipe = controller.form["recipe"] }
            }
    }
}

/// The same section order as the shared profile menus; ICC loading stays off UI.
private struct PrintProfileMenu: View {
    @ObservedObject var controller: ProofController
    let recipe: JSON
    @Binding var busy: Bool
    let choose: (JSON) -> Void
    @State private var saved: [JSON] = []
    @State private var importing = false
    @State private var library = false
    @State private var mounted = false
    var body: some View {
        HStack {
            Text("Profile")
            Spacer(minLength: 4)
            Menu {
                if !controller.form["document_profile"].isNull {
                    Section("Document Profile") { Button(controller.form["document_profile"]["name"].string) { choose(controller.form["document_profile"]) } }
                }
                if !saved.isEmpty {
                    Section("Saved Profiles") { ForEach(saved, id: \.stableKey) { p in Button(p["name"].string) { read(id: p["id"].string) } } }
                }
                Section("Standard Color Spaces") {
                    ForEach(controller.form["profiles"].array, id: \.stableKey) { p in Button(p["name"].string) { choose(p) } }
                }
                Divider()
                Button("Add ICC Profile…") { importing = true }
                Button("Manage Profiles…") { library = true }
            } label: { Text(recipe["name"].string.isEmpty ? "Choose a profile" : recipe["name"].string).lineLimit(1) }
                .accessibilityIdentifier("proof-profile")
        }
        .onAppear { mounted = true; reload() }.onDisappear { mounted = false }
        .sheet(isPresented: $library, onDismiss: reload) {
            ColorProfileLibrary(preferences: controller.preferences) { choose($0); library = false }
                .modifier(EditorPopupPresentation())
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [UTType(filenameExtension: "icc") ?? .data, UTType(filenameExtension: "icm") ?? .data]) { result in
            switch result {
            case .success(let url): read(url: url)
            case .failure(let failure):
                let e = failure as NSError
                if e.domain != NSCocoaErrorDomain || e.code != NSUserCancelledError { controller.error = failure.localizedDescription }
            }
        }
    }
    private func reload() {
        let preferences = controller.preferences
        NativeProjectTask.io.async {
            let result = Result { try preferences.profiles() }
            DispatchQueue.main.async {
                guard mounted else { return }
                switch result { case .success(let entries): saved = entries.filter { $0["visible"].bool && $0["issue"].isNull }
                case .failure(let error): controller.error = error.localizedDescription }
            }
        }
    }
    private func read(id: String? = nil, url: URL? = nil) {
        busy = true
        let preferences = controller.preferences
        NativeProjectTask.io.async {
            let result = Result { if let url { return try preferences.importProfile(url) }; return try preferences.profile(id!) }
            DispatchQueue.main.async {
                busy = false
                guard mounted else { return }
                switch result { case .success(let profile): choose(profile); reload()
                case .failure(let error): controller.error = error.localizedDescription }
            }
        }
    }
}
