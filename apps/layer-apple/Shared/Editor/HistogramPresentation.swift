import SwiftUI

/// A nonmodal inspector leaves the canvas reachable while analysis runs.
struct HistogramPresentation: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    @ObservedObject var model: HistogramController
    let palette: EditorPalette
    @State private var channel = 0
    @State private var logarithmic = false
    @State private var details = false
    private var indices: [Int] { channel == 0 ? [0, 1, 2] : [channel - 1] }
    private var histogram: JSON { model.result["histogram"] }
    var body: some View {
        if model.isOpen {
            GeometryReader { geometry in
                EditorScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            Text(model.copy["histogram"].string).font(.headline)
                            Spacer()
                            Button(model.common["close"].string, action: model.close).accessibilityIdentifier("histogram-close")
                        }
                        Picker(model.copy["channel"].string, selection: $channel) {
                            ForEach(Array(["RGB", model.copy["red"].string, model.copy["green"].string, model.copy["blue"].string, model.copy["luminance"].string].enumerated()), id: \.offset) { index, title in
                                Text(title).tag(index)
                            }
                        }.accessibilityIdentifier("histogram-channel")
                        HStack {
                            Toggle(model.copy["log_counts"].string, isOn: $logarithmic)
                            Toggle(model.copy["auto_update"].string, isOn: $model.automatic)
                        }
                        HistogramChart(label: model.copy["distribution"].string, channels: histogram["channels"].array, indices: indices, logarithmic: logarithmic, axis: model.result["axis"])
                            .frame(height: 140).background(palette["bg"])
                        HistogramAxis(axis: model.result["axis"]).frame(height: 18)
                        Button { details.toggle() } label: {
                            HStack(spacing: 4) {
                                SharedIcon(name: "chevron-down", size: 12).rotationEffect(.degrees(details ? 0 : -90))
                                Text(model.copy["details"].string)
                            }.contentShape(Rectangle())
                        }.buttonStyle(.plain).accessibilityIdentifier("histogram-details")
                            .accessibilityValue(details ? nativeCopy["layers"]["expanded"].string : nativeCopy["layers"]["collapsed"].string)
                        if details {
                        if !histogram.isNull {
                            let color = histogram["color"]
                            Text("\(color["space"].string) · \(color["depth"].string == "F32" ? "32-bit float HDR" : color["depth"].string == "F16" ? "16-bit float HDR" : color["depth"].string == "U16" ? "16-bit" : "8-bit") · \(histogram["pixels"].uint) nontransparent pixels")
                                .accessibilityIdentifier("histogram-summary")
                            ForEach(indices, id: \.self) { i in
                                let c = histogram["channels"][i]
                                Text(model.result["axis"]["hdr"].bool ? "\(["R", "G", "B", "Y"][i]): nonpositive \(c["black"].uint) · above SDR white \(c["above"].uint)" : "\(["R", "G", "B", "Y"][i]): below 0 \(c["below"].uint), above 1 \(c["above"].uint) · black \(c["black"].uint), white \(c["white"].uint)")
                                    .font(.caption).accessibilityIdentifier("histogram-channel-\(i)")
                            }
                        }
                        Text(model.result["axis"]["hdr"].bool ? model.result["axis"]["description"].string + ". Includes visible paper; excludes transparent pixels and display overlays." : model.copy["inspection_help"].string)
                            .font(.caption)
                        }
                        Text(model.stale && !model.busy ? model.staleStatus : model.status)
                            .accessibilityIdentifier("histogram-status")
                        Button(model.copy["refresh"].string, action: model.refresh).disabled(model.busy).accessibilityIdentifier("histogram-refresh")
                    }.padding(16)
                }
                .frame(width: min(380, max(0, geometry.size.width - 24)))
                .frame(maxHeight: max(0, geometry.size.height - 100))
                .fixedSize(horizontal: false, vertical: true)
                .background(SquircleShape(12).fill(palette["panel"]).shadow(radius: 6, y: 2))
                .overlay(SquircleShape(12).strokeBorder(palette["border"], lineWidth: 1))
                .padding(.top, 76).padding(.trailing, 12)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
            }.onDisappear(perform: model.close)
        }
    }
}

private struct HistogramChart: View {
    let label: String
    let channels: [JSON]
    let indices: [Int]
    let logarithmic: Bool
    let axis: JSON
    var body: some View {
        Canvas { context, size in
            let values = indices.map { i in
                (channels.indices.contains(i) ? channels[i]["bins"].array : []).map { logarithmic ? log1p($0.number) : $0.number }
            }
            let first = Int(axis["start"].uint), end = axis["end"].isNull ? 256 : Int(axis["end"].uint)
            let range = first..<max(first + 1, end)
            let plotted = values.map { v in Array(v.dropFirst(first).prefix(range.count)) }
            let maximum = max(1, plotted.flatMap { $0 }.max() ?? 0)
            if let white = axis["ticks"].array.first(where: { $0["white"].bool }) {
                let x = white["fraction"].number * size.width
                context.fill(Path(CGRect(x: x, y: 0, width: size.width - x, height: size.height)), with: .color(.white.opacity(0.06)))
                var p = Path(); p.move(to: CGPoint(x: x, y: 0)); p.addLine(to: CGPoint(x: x, y: size.height))
                context.stroke(p, with: .color(.primary.opacity(0.5)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
            }
            let colors: [Color] = [Color(red: 0.93, green: 0.45, blue: 0.45), Color(red: 0.41, green: 0.81, blue: 0.57), Color(red: 0.45, green: 0.65, blue: 0.96), .gray]
            for (position, i) in indices.enumerated() where values[position].count == 256 {
                var path = Path()
                path.move(to: CGPoint(x: 0, y: size.height))
                for (x, value) in plotted[position].enumerated() {
                    path.addLine(to: CGPoint(x: CGFloat(x) / CGFloat(max(1, range.count - 1)) * size.width, y: size.height * CGFloat(1 - value / maximum)))
                }
                path.addLine(to: CGPoint(x: size.width, y: size.height)); path.closeSubpath()
                context.fill(path, with: .color(colors[i].opacity(i == 3 ? 0.8 : 0.53)))
            }
        }.accessibilityLabel(label)
    }
}

private struct HistogramAxis: View {
    let axis: JSON
    var body: some View {
        GeometryReader { geometry in
            ForEach(axis["ticks"].array, id: \.stableKey) { tick in
                Text(tick["label"].string).font(.system(size: 9))
                    .position(x: min(geometry.size.width - 12, max(12, tick["fraction"].number * geometry.size.width)), y: 8)
            }
        }
    }
}
