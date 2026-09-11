import SwiftUI

struct NavigatorPanel: View {
    @ObservedObject var store: EditorStore
    @State private var viewer = UUID()
    private let commands = ["zoom_out", "zoom_in", "rotate_left", "rotate_right", "flip_horizontal", "flip_vertical"]
    var body: some View {
        VStack(spacing: 2) {
            NavigatorDrawing(store: store, camera: store.camera, images: store.navigatorImages)
                .frame(minHeight: 0, idealHeight: 164, maxHeight: .infinity)
            HStack(spacing: 2) {
                ForEach(commands, id: \.self) { id in
                    let command = store.command(id)
                    IconTile(icon: command["icon"].string, label: command["tooltip"].string,
                        selected: command["selected"].bool, enabled: command["enabled"].bool) { store.invoke(id) }
                        .frame(height: 32).accessibilityIdentifier("navigator-" + id)
                        .accessibilityAddTraits(command["selected"].bool ? .isSelected : [])
                }
            }
        }.padding(8).onAppear { store.navigatorImages.show(viewer) }
            .onDisappear { store.navigatorImages.hide(viewer) }
    }
}

private struct NavigatorDrawing: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var camera: CameraReadout
    @ObservedObject var images: NavigatorImages
    @State private var started = false
    @GestureState private var contact = false
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func geometry(_ size: CGSize) -> JSON {
        let document = store.state["tabs"][0]
        guard size.width > 8, size.height > 8, !camera.value.isNull,
            let source = try? JSON([camera.value.raw, [document["width"].uint, document["height"].uint], [size.width, size.height]]).encoded(),
            let pointer = source.withCString({ capy_apple_navigator_geometry($0) }) else { return JSON() }
        defer { capy_apple_string_free(pointer) }
        return (try? JSON.decode(String(cString: pointer))) ?? JSON()
    }
    private func send(_ phase: String, _ point: CGPoint = .zero, _ size: CGSize = .zero) {
        store.dispatch(["type": "navigator", "phase": phase, "position": [point.x, point.y], "viewport": [size.width, size.height]])
    }
    var body: some View {
        GeometryReader { allocation in
            let model = geometry(allocation.size)
            Canvas { context, _ in
                guard !model.isNull else { return }
                let imageBounds = model["image"].rect
                if let image = images.image { context.draw(Image(decorative: image, scale: 1), in: imageBounds) }
                context.clip(to: Path(imageBounds))
                var outline = Path()
                for (index, point) in model["work_area"].array.enumerated() {
                    let point = CGPoint(x: point[0].number, y: point[1].number)
                    if index == 0 { outline.move(to: point) } else { outline.addLine(to: point) }
                }
                outline.closeSubpath()
                context.stroke(outline, with: .color(.white), lineWidth: 3)
                context.stroke(outline, with: .color(Color(red: 53 / 255, green: 132 / 255, blue: 228 / 255)), lineWidth: 1.5)
            }.background(palette["bg"]).contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).updating($contact) { _, value, _ in value = true }.onChanged { event in
                    if !started { started = true; send("down", event.startLocation, allocation.size) }
                    if event.translation != .zero { send("move", event.location, allocation.size) }
                }.onEnded { event in
                    if started { send("up", event.location, allocation.size); started = false }
                })
                .onChange(of: contact) { _, active in if !active && started { send("cancel"); started = false } }
                .onDisappear { if started { send("cancel"); started = false } }
                .accessibilityElement().accessibilityLabel("Navigator")
                .accessibilityValue(images.image == nil ? "Preview pending" : "Preview ready")
                .accessibilityHint("Drag the work area to move the canvas")
                .accessibilityIdentifier("navigator-overview")
        }
    }
}

struct RendererStatsPanel: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var stats: RendererStats
    @State private var viewer = UUID()
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        VStack(spacing: 6) {
            ForEach(stats.view["rows"].array.indices, id: \.self) { index in
                let row = stats.view["rows"][index]
                HStack(alignment: .firstTextBaseline) {
                    Text(row["label"].string)
                    Spacer(minLength: 4)
                    Text(row["value"].string).monospacedDigit().fixedSize()
                        .accessibilityIdentifier("stats-value-\(index)")
                }.help(row["description"].string)
            }
            Canvas { context, size in
                let samples = stats.view["samples"].array.map(\.number)
                let budget = stats.view["budget_ms"].number
                let maximum = max(0.001, max(budget, samples.max() ?? 0) * 1.1)
                let y = size.height * (1 - budget / maximum)
                var line = Path(); line.move(to: CGPoint(x: 0, y: y)); line.addLine(to: CGPoint(x: size.width, y: y))
                context.stroke(line, with: .color(palette["text"].opacity(0.55)), lineWidth: 1)
                var chart = Path()
                for (index, value) in samples.enumerated() {
                    let point = CGPoint(x: Double(index) * size.width / 119, y: size.height * (1 - value / maximum))
                    if index == 0 { chart.move(to: point) } else { chart.addLine(to: point) }
                }
                context.stroke(chart, with: .color(palette["text"]), lineWidth: 1)
            }.frame(height: 46).accessibilityElement().accessibilityLabel(stats.view["chart_label"].string)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("renderer-stats")
            .onAppear { stats.show(viewer) }.onDisappear { stats.hide(viewer) }
    }
}
