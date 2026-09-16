import SwiftUI

/// A nonmodal inspector leaves the canvas reachable while analysis runs.
struct HistogramPresentation: View {
    @ObservedObject var model: HistogramController
    let palette: EditorPalette
    @State private var channel = 0
    @State private var logarithmic = false
    private var indices: [Int] { channel == 0 ? [0, 1, 2] : [channel - 1] }
    private var histogram: JSON { model.result["histogram"] }
    var body: some View {
        if model.isOpen {
            GeometryReader { geometry in
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            Text("Histogram").font(.headline)
                            Spacer()
                            Button("Close", action: model.close).accessibilityIdentifier("histogram-close")
                        }
                        Picker("Channel", selection: $channel) {
                            ForEach(Array(["RGB", "Red", "Green", "Blue", "Luminance"].enumerated()), id: \.offset) { index, title in
                                Text(title).tag(index)
                            }
                        }.accessibilityIdentifier("histogram-channel")
                        HStack {
                            Toggle("Log scale", isOn: $logarithmic)
                            Toggle("Auto update", isOn: $model.automatic)
                        }
                        HistogramChart(channels: histogram["channels"].array, indices: indices, logarithmic: logarithmic)
                            .frame(height: 140).background(palette["bg"])
                        if !histogram.isNull {
                            let color = histogram["color"]
                            Text("\(color["space"].string) · \(color["depth"].string == "U16" ? 16 : 8)-bit · \(histogram["pixels"].uint) nontransparent pixels")
                                .accessibilityIdentifier("histogram-summary")
                            ForEach(indices, id: \.self) { i in
                                let c = histogram["channels"][i]
                                Text("\(["R", "G", "B", "Y"][i]): below 0 \(c["below"].uint), above 1 \(c["above"].uint) · black \(c["black"].uint), white \(c["white"].uint)")
                                    .font(.caption).accessibilityIdentifier("histogram-channel-\(i)")
                            }
                        }
                        Text(model.stale && !model.busy ? "Drawing changed · showing previous inspection. \(model.status)" : model.status)
                            .accessibilityIdentifier("histogram-status")
                        Text("Encoded document RGB · linear luminance Y. Includes visible paper; excludes transparent pixels and display overlays.")
                            .font(.caption)
                        Button("Refresh", action: model.refresh).disabled(model.busy).accessibilityIdentifier("histogram-refresh")
                    }.padding(16)
                }
                .frame(width: min(380, max(0, geometry.size.width - 24)))
                .frame(maxHeight: max(0, geometry.size.height - 100))
                .fixedSize(horizontal: false, vertical: true)
                .background(RoundedRectangle(cornerRadius: 12).fill(palette["panel"]).shadow(radius: 6, y: 2))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(palette["border"], lineWidth: 1))
                .padding(.top, 76).padding(.trailing, 12)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
            }.onDisappear(perform: model.close)
        }
    }
}

private struct HistogramChart: View {
    let channels: [JSON]
    let indices: [Int]
    let logarithmic: Bool
    var body: some View {
        Canvas { context, size in
            let values = indices.map { i in
                (channels.indices.contains(i) ? channels[i]["bins"].array : []).map { logarithmic ? log1p($0.number) : $0.number }
            }
            let maximum = max(1, values.flatMap { $0 }.max() ?? 0)
            let colors: [Color] = [Color(red: 0.93, green: 0.45, blue: 0.45), Color(red: 0.41, green: 0.81, blue: 0.57), Color(red: 0.45, green: 0.65, blue: 0.96), .gray]
            for (position, i) in indices.enumerated() where values[position].count == 256 {
                var path = Path()
                path.move(to: CGPoint(x: 0, y: size.height))
                for (x, value) in values[position].enumerated() {
                    path.addLine(to: CGPoint(x: CGFloat(x) / 255 * size.width, y: size.height * CGFloat(1 - value / maximum)))
                }
                path.addLine(to: CGPoint(x: size.width, y: size.height)); path.closeSubpath()
                context.fill(path, with: .color(colors[i].opacity(i == 3 ? 0.8 : 0.53)))
            }
        }.accessibilityLabel("Histogram distribution")
    }
}
