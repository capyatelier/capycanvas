import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

struct ToolbarComponentView: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let tile: JSON
    let size: CGSize
    let vertical: Bool
    var body: some View {
        Group {
            if tile["control"]["kind"].string == "tool_options" {
                ToolOptionsComponent(store: store, panel: panel, tile: tile, size: size, vertical: vertical)
            } else {
                BrushSliderComponent(store: store, panel: panel, tile: tile, size: size, vertical: vertical)
            }
        }.id(tile["component"]["context"].stableKey)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("toolbar-component-\(tile["id"].uint)")
    }
}

private func toolbarItem(_ panel: JSON, _ tile: JSON) -> JSON {
    JSON(["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw])
}

extension EditorStore {
    func toolbarEdit(_ component: JSON, _ action: Any, completion: @escaping @MainActor (String?) -> Void = { _ in }) {
        edit(["type": "toolbar_edit", "context": component["context"].raw, "action": action], completion: completion)
    }
}

private func textWidth(_ text: String, size: CGFloat, bold: Bool = false) -> CGFloat {
    #if canImport(AppKit)
    let font = NSFont.systemFont(ofSize: size, weight: bold ? .bold : .regular)
    #else
    let font = UIFont.systemFont(ofSize: size, weight: bold ? .bold : .regular)
    #endif
    return ceil((text as NSString).size(withAttributes: [.font: font]).width)
}

private struct BrushSliderComponent: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let tile: JSON
    let size: CGSize
    let vertical: Bool
    @StateObject private var session = SliderPreviewSession()
    @State private var frame = CGRect.zero
    private var model: JSON { tile["component"] }
    private var field: JSON { model["numeric"] }
    private var spec: JSON {
        field.isNull ? ToolbarUI.cached(["type": "slider_spec", "control": tile["control"].raw]) : field["numeric"]
    }
    private var value: Double { field.isNull ? spec["min"].number : field["value"].number }
    private var label: String { field.isNull ? tile["label"].string : field["label"].string }
    private var setting: String {
        field.isNull ? (tile["control"]["kind"].string == "brush_opacity_slider" ? "opacity" : "size") : field["id"].string
    }
    var body: some View {
        let item = toolbarItem(panel, tile)
        let layout = ToolbarUI.cached(["type": "slider_layout", "width": size.width, "height": size.height,
            "axis": vertical ? "vertical" : "horizontal"])
        let shown = ToolbarUI.formatted(spec, value: value)
        let marks = model["bookmarks"].array
        ZStack(alignment: .topLeading) {
            Button {
                guard !store.workspace.input.contact.consumeClick() else { return }
                openPreview()
            } label: { Color.clear.contentShape(Rectangle()) }
                .buttonStyle(.plain).disabled(field.isNull)
                .accessibilityLabel(label).help(label)
                .accessibilityIdentifier("slider-cap-\(tile["id"].uint)")
                .modifier(WorkspaceDrag(workspace: store.workspace, item: item, surface: .tile, context: item))
                .placed(layout[0])
            ToolbarSliderTrack(fill: shown["fill"].number, marks: marks,
                opacity: tile["control"]["kind"].string == "brush_opacity_slider", vertical: vertical,
                enabled: !field.isNull, label: label, valueText: shown["text"].string,
                palette: EditorPalette(source: store.state["palette"]),
                contact: { down, moved in
                    if down { openPreview() } else if moved { session.close(store.workspace.sliderPreview) }
                }, change: pick, step: step)
                .modifier(WorkspaceControlSurface(workspace: store.workspace))
                .accessibilityIdentifier("component-slider-\(tile["id"].uint)")
                .placed(layout[1])
        }
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("editor-workspace")) } action: { next in
            if next.size != frame.size { session.close(store.workspace.sliderPreview) }
            frame = next; refreshPreview()
        }
        .onChange(of: JSON([value, marks.map(\.raw)]).stableKey) { _, _ in refreshPreview() }
        .onDisappear { session.close(store.workspace.sliderPreview) }
    }
    private func pick(_ fill: Double, _ snap: Bool, _ travel: Double) {
        guard !field.isNull else { return }
        let next: Double
        if snap {
            next = ToolbarUI.resolve(["type": "slider_bookmark_value", "control": tile["control"].raw,
                "values": model["bookmarks"].array.map { $0["value"].number }, "position": fill, "travel": travel]).number
        } else {
            guard let resolved = try? store.resolveNumber(spec, value: value, operation: ["type": "position", "position": fill]) else { return }
            next = resolved["value"].number
        }
        store.toolbarEdit(model, ["type": "set_tool_setting", "id": setting, "value": next])
    }
    private func step(_ direction: Int) {
        guard !field.isNull,
              let resolved = try? store.resolveNumber(spec, value: value, operation: ["type": "step", "steps": direction]) else { return }
        store.toolbarEdit(model, ["type": "set_tool_setting", "id": setting, "value": resolved["value"].number])
    }
    private func openPreview() {
        guard !field.isNull else { return }
        let context = model["context"]
        if session.stamp == nil || session.context != context.stableKey {
            let request = UUID(); session.request = request
            store.query(["type": "toolbar_stamp", "context": context.raw]) { reply in
                guard session.request == request, let image = SliderPreviewSession.image(reply) else { return }
                session.stamp = (image, reply["extent"].number); session.context = context.stableKey
                session.open = true; refreshPreview()
            }
        } else { session.open = true; refreshPreview() }
    }
    private func refreshPreview() {
        guard session.open, let stamp = session.stamp else { return }
        let geometry = ToolbarUI.resolve(["type": "slider_preview", "control": tile["control"].raw, "style": panel["tile_style"].raw,
            "value": value, "length": max(size.width, size.height), "extent": stamp.extent])
        guard geometry["error"].isNull else { return }
        let control = tile["control"].raw, model = model, store = store
        store.workspace.sliderPreview.show(.init(owner: session.id, anchor: frame, vertical: vertical,
            image: stamp.image, geometry: geometry,
            selected: model["bookmarks"].array.contains { $0["selected"].bool },
            bookmark: { store.toolbarEdit(model, ["type": "toggle_slider_bookmark", "control": control]) },
            dismissed: { [weak session] in session?.open = false }))
    }
}

@MainActor private final class SliderPreviewSession: ObservableObject {
    let id = UUID()
    var open = false
    var request: UUID?
    var context = ""
    var stamp: (image: CGImage, extent: Double)?
    func close(_ preview: ToolbarSliderPreview) {
        request = nil
        if open { open = false; preview.close(id) }
    }
    static func image(_ reply: JSON) -> CGImage? {
        let side = Int(reply["size"].uint), alpha = reply["alpha"].array
        guard side > 0, alpha.count == side * side else { return nil }
        var bytes = [UInt8](repeating: 0, count: side * side * 4)
        for (index, value) in alpha.enumerated() {
            let a = UInt8(clamping: Int(value.uint))
            bytes[index * 4] = a; bytes[index * 4 + 1] = a; bytes[index * 4 + 2] = a; bytes[index * 4 + 3] = a
        }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(width: side, height: side, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: side * 4,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }
}

private struct ToolbarSliderTrack: View {
    let fill: Double
    let marks: [JSON]
    let opacity: Bool
    let vertical: Bool
    let enabled: Bool
    let label: String
    let valueText: String
    let palette: EditorPalette
    let contact: (_ down: Bool, _ moved: Bool) -> Void
    let change: (_ fill: Double, _ snap: Bool, _ travel: Double) -> Void
    let step: (Int) -> Void
    @State private var origin: CGPoint?
    @State private var moved = false
    @GestureState private var tracking = false
    private var insets: EdgeInsets {
        vertical ? EdgeInsets(top: 2, leading: 5, bottom: 2, trailing: 5) : EdgeInsets(top: 5, leading: 2, bottom: 5, trailing: 2)
    }
    var body: some View {
        GeometryReader { geometry in
            let area = CGSize(width: max(0, geometry.size.width - insets.leading - insets.trailing),
                height: max(0, geometry.size.height - insets.top - insets.bottom))
            Canvas { context, _ in
                var inset = context
                inset.translateBy(x: insets.leading, y: insets.top)
                draw(inset, area)
            }
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).updating($tracking) { _, state, _ in state = true }
                    .onChanged { event in
                        guard enabled else { return }
                        let local = CGPoint(x: event.location.x - insets.leading, y: event.location.y - insets.top)
                        if origin == nil {
                            origin = event.location; moved = false
                            contact(true, false); pick(local, area, snap: true)
                        } else if let origin, moved || hypot(event.location.x - origin.x, event.location.y - origin.y) >= 3 {
                            moved = true; pick(local, area, snap: false)
                        }
                    }.onEnded { _ in
                        guard origin != nil else { return }
                        origin = nil; contact(false, moved)
                    }, isEnabled: enabled)
        }
        .opacity(enabled ? 1 : 0.4)
        .onChange(of: tracking) { _, active in
            if !active && origin != nil { origin = nil; contact(false, true) }
        }
        .accessibilityElement().accessibilityLabel(label).accessibilityValue(valueText)
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: step(1)
            case .decrement: step(-1)
            @unknown default: break
            }
        }
    }
    private func pick(_ point: CGPoint, _ area: CGSize, snap: Bool) {
        let length = (vertical ? area.height : area.width) - 12
        guard length > 0 else { return }
        let fill = vertical ? 1 - (point.y - 6) / length : (point.x - 6) / length
        change(min(1, max(0, fill)), snap, length)
    }
    private func draw(_ context: GraphicsContext, _ size: CGSize) {
        let text = palette["text"]
        let length = (vertical ? size.height : size.width) - 12
        if length >= 4 {
            var track = context
            if vertical {
                track.translateBy(x: size.width / 2, y: size.height - 6); track.rotate(by: .degrees(-90))
            } else { track.translateBy(x: 6, y: size.height / 2) }
            let wide: CGFloat = 8, narrow: CGFloat = opacity ? wide : 2.5
            var path = Path()
            path.move(to: CGPoint(x: 0, y: -narrow)); path.addLine(to: CGPoint(x: length - 3, y: -wide))
            path.addCurve(to: CGPoint(x: length - 3, y: wide), control1: CGPoint(x: length + 1, y: -wide), control2: CGPoint(x: length + 1, y: wide))
            path.addLine(to: CGPoint(x: 0, y: narrow))
            path.addCurve(to: CGPoint(x: 0, y: -narrow), control1: CGPoint(x: -3, y: narrow), control2: CGPoint(x: -3, y: -narrow))
            path.closeSubpath()
            track.clip(to: path)
            let bounds = CGRect(x: -3, y: -wide, width: length + 4, height: wide * 2)
            if opacity {
                track.fill(Path(bounds), with: .color(text.opacity(0.08)))
                let cell: CGFloat = 4
                for column in -1...(Int(length / cell) + 1) {
                    for row in -2...1 where (column + row) % 2 == 0 {
                        track.fill(Path(CGRect(x: CGFloat(column) * cell, y: CGFloat(row) * cell, width: cell, height: cell)),
                            with: .color(text.opacity(0.2)))
                    }
                }
                track.fill(Path(bounds), with: .linearGradient(Gradient(colors: [text.opacity(0), text.opacity(0.65)]),
                    startPoint: .zero, endPoint: CGPoint(x: length, y: 0)))
            } else {
                track.fill(Path(bounds), with: .color(text.opacity(0.22)))
            }
        }
        let along: CGFloat = 12, across: CGFloat = 28
        let thumb = vertical
            ? CGRect(x: (size.width - across) / 2, y: (size.height - along) * (1 - fill), width: across, height: along)
            : CGRect(x: (size.width - along) * fill, y: (size.height - across) / 2, width: along, height: across)
        context.fill(SquircleShape(6).path(in: thumb), with: .color(palette["thumb"]))
        context.stroke(SquircleShape(5.5).path(in: thumb.insetBy(dx: 0.5, dy: 0.5)), with: .color(text.opacity(0.6)), lineWidth: 1)
        for mark in marks {
            let position = mark["selected"].bool ? fill : mark["fill"].number
            let center = vertical
                ? CGPoint(x: size.width / 2, y: along / 2 + (size.height - along) * (1 - position))
                : CGPoint(x: along / 2 + (size.width - along) * position, y: size.height / 2)
            var line = Path()
            if vertical {
                line.move(to: CGPoint(x: center.x - 7, y: center.y)); line.addLine(to: CGPoint(x: center.x + 7, y: center.y))
            } else {
                line.move(to: CGPoint(x: center.x, y: center.y - 7)); line.addLine(to: CGPoint(x: center.x, y: center.y + 7))
            }
            context.stroke(line, with: .color(mark["selected"].bool ? palette["panel"] : text), lineWidth: 2)
        }
    }
}

@MainActor final class ToolbarSliderPreview: ObservableObject {
    struct Content {
        let owner: UUID
        let anchor: CGRect
        let vertical: Bool
        let image: CGImage
        let geometry: JSON
        let selected: Bool
        let bookmark: () -> Void
        let dismissed: () -> Void
    }
    @Published private(set) var content: Content?
    var frame = CGRect.zero
    func show(_ next: Content) { content = next }
    func close(_ owner: UUID) { if content?.owner == owner { content = nil; frame = .zero } }
    func dismiss(outside point: CGPoint?) {
        guard let content else { return }
        if let point, frame.contains(point) || content.anchor.contains(point) { return }
        self.content = nil; frame = .zero; content.dismissed()
    }
}

struct ToolbarSliderPreviewOverlay: View {
    @ObservedObject var preview: ToolbarSliderPreview
    let palette: EditorPalette
    var body: some View {
        GeometryReader { viewport in
            if let content = preview.content {
                let side = content.geometry["side"].number, a = content.anchor
                let x = content.vertical ? (a.maxX + side + 8 <= viewport.size.width ? a.maxX + 8 : a.minX - side - 8) : a.minX
                let y = content.vertical ? a.minY + (a.height - side) / 2
                    : (a.maxY + side + 8 <= viewport.size.height ? a.maxY + 8 : a.minY - side - 8)
                let origin = CGPoint(x: max(6, min(x, viewport.size.width - side - 6)), y: max(6, min(y, viewport.size.height - side - 6)))
                let geometry = content.geometry, shape = SquircleShape(geometry["radius"].number)
                ZStack(alignment: .topLeading) {
                    Canvas { context, size in draw(context, size, content) }
                    Text(geometry["text"].string).lineLimit(1).fixedSize().accessibilityIdentifier("slider-preview-caption")
                        .frame(width: geometry["caption"]["width"].number, height: geometry["caption"]["height"].number, alignment: .leading)
                        .offset(x: geometry["caption"]["x"].number, y: geometry["caption"]["y"].number)
                    Button(action: content.bookmark) {
                        SharedIcon(name: content.selected ? "minus" : "plus", size: geometry["icon"].number)
                            .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                    }.buttonStyle(EditorControlButtonStyle(corner: .half))
                        .accessibilityLabel(content.selected ? "Remove bookmark" : "Bookmark this value")
                        .help(content.selected ? "Remove bookmark" : "Bookmark this value")
                        .accessibilityIdentifier("slider-bookmark")
                        .placed(geometry["bookmark"])
                }
                .frame(width: side, height: side)
                .background(palette["panel"], in: shape)
                .clipShape(shape)
                .shadow(color: .black.opacity(0.18), radius: 6, y: 2)
                .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("editor-workspace")) } action: { preview.frame = $0 }
                .offset(x: origin.x, y: origin.y)
                .accessibilityElement(children: .contain).accessibilityIdentifier("brush-slider-preview")
            }
        }
    }
    private func draw(_ context: GraphicsContext, _ size: CGSize, _ content: Content) {
        let geometry = content.geometry
        let viewport = geometry["viewport"].rect, stamp = geometry["stamp"].rect
        var layer = context
        layer.clip(to: Path(viewport))
        layer.opacity = geometry["opacity"].number
        layer.clipToLayer { mask in mask.draw(Image(decorative: content.image, scale: 1), in: stamp) }
        layer.fill(Path(stamp), with: .color(palette["text"]))
        let fade = geometry["header_fade"].number
        if fade > 0 {
            context.fill(Path(CGRect(x: 0, y: 0, width: size.width, height: fade)), with: .linearGradient(
                Gradient(colors: [palette["panel"].opacity(geometry["header_fade_opacity"].number), palette["panel"].opacity(0)]),
                startPoint: .zero, endPoint: CGPoint(x: 0, y: fade)))
        }
    }
    typealias Content = ToolbarSliderPreview.Content
}

private struct ToolOptionsComponent: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let tile: JSON
    let size: CGSize
    let vertical: Bool
    private var model: JSON { tile["component"] }
    private var preferences: JSON { tile["control"]["style"] }
    private var textSize: CGFloat { store.catalog["text_size_pt"].number * 4 / 3 }
    var body: some View {
        let item = toolbarItem(panel, tile)
        let style = ToolbarUI.cached(["type": "style", "style": panel["tile_style"].raw])
        let tileSize = CGSize(width: style["size"][0].number, height: style["size"][1].number)
        let options = model["options"].array
        let sizes = options.map { fieldSize($0, tile: tileSize) }
        let layout = ToolbarUI.cached(["type": "options_layout", "width": size.width, "height": size.height,
            "axis": vertical ? "vertical" : "horizontal", "sizes": sizes.map { [$0.width, $0.height] },
            "button": style["size"].raw, "gap": vertical ? style["gap"].number : 10])
        ZStack(alignment: .topLeading) {
            Color.clear.contentShape(Rectangle())
                .modifier(WorkspaceDrag(workspace: store.workspace, item: item, surface: .tile, context: item, canDrag: false))
            ForEach(options.indices, id: \.self) { index in
                let bounds = layout["fields"][index]
                if !bounds.isNull {
                    ToolOptionField(store: store, panel: panel, option: options[index], component: model, vertical: vertical,
                        labeled: style["labeled"].bool, style: panel["tile_style"].string, preferences: preferences,
                        stacked: bounds["width"].number < tileSize.width * CGFloat(options[index]["Choice"]["items"].array.count))
                        .frame(width: bounds["width"].number, height: bounds["height"].number)
                        .modifier(WorkspaceControlSurface(workspace: store.workspace))
                        .offset(x: bounds["x"].number, y: bounds["y"].number)
                }
            }
            ToolOptionsMore(store: store, panel: panel, tile: tile, item: item)
                .placed(layout["more"])
        }
    }
    private func fieldSize(_ option: JSON, tile: CGSize) -> CGSize {
        if !option["Range"].isNull { return CGSize(width: preferences["sliders"].bool ? 280 : 100, height: 28) }
        let choice = option["Choice"]
        if !choice.isNull && choice["segmented"].bool {
            let count = CGFloat(choice["items"].array.count)
            return vertical ? CGSize(width: size.width, height: tile.height * (size.width < tile.width * count ? count : 1))
                : CGSize(width: tile.width * count, height: 24)
        }
        if vertical || !option["Action"].isNull { return tile }
        if !choice.isNull { return CGSize(width: 168, height: 24) }
        let field = option["Numeric"]
        let samples = ToolbarUI.cached(["type": "numeric_info", "id": field["id"].raw, "control": field["numeric"].raw,
            "compact": true, "units": true])["samples"].array
        let valueWidth = (samples.map { textWidth($0.string.map { $0.isNumber ? "8" : String($0) }.joined(), size: textSize) }.max() ?? 0) + 14
        let labelWidth = preferences["text"].bool ? textWidth(field["label"].string, size: textSize) : 16
        return CGSize(width: labelWidth + 4 + valueWidth + (preferences["sliders"].bool ? 60 : 0), height: 24)
    }
}

private struct ToolOptionsMore: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let tile: JSON
    let item: JSON
    var body: some View {
        let source = store.contentDrawers.sources["tool"]
        button(joined: source?.opens(tile: tile, in: panel) == true ? source?.direction : nil)
    }
    private func button(joined: String?) -> some View {
        Button {
            guard !store.workspace.input.contact.consumeClick() else { return }
            store.dispatch(["type": "activate_tile", "panel": panel["id"].raw, "tile": tile["id"].raw])
        } label: {
            SharedIcon(name: "more", size: CGFloat(panel["tile_icon_size"].number))
                .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle(joinedEdge: joined,
            drawerBackground: joined == nil ? nil : .clear, corner: .half))
            .accessibilityLabel("More tool options").help("More tool options")
            .accessibilityIdentifier("toolbar-more-\(tile["id"].uint)")
            .modifier(WorkspaceDrag(workspace: store.workspace, item: item, surface: .tile, context: item))
    }
}

private struct ToolOptionField: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let option: JSON
    let component: JSON
    let vertical: Bool
    let labeled: Bool
    let style: String
    let preferences: JSON
    let stacked: Bool
    var body: some View {
        if !option["Numeric"].isNull {
            ToolbarNumberField(store: store, field: option["Numeric"], component: component, vertical: vertical,
                labeled: labeled, style: style, preferences: preferences)
        } else if !option["Choice"].isNull {
            ToolbarChoiceField(store: store, choice: option["Choice"], component: component, vertical: vertical,
                labeled: labeled, stacked: stacked, iconSize: CGFloat(panel["tile_icon_size"].number))
        } else if !option["Range"].isNull {
            let range = option["Range"], bounds = range["bounds"].array
            RangeControl(store: store, bounds: bounds, label: range["label"].string, prefix: "toolbar",
                showSlider: preferences["sliders"].bool) { index, value, completion in
                store.toolbarEdit(component, ["type": "set_tool_setting", "id": bounds[index]["id"].raw, "value": value], completion: completion)
            }
        } else if !option["Action"].isNull {
            let command = option["Action"]["state"]
            Button { store.toolbarEdit(component, ["type": "invoke", "command": command["id"].raw]) } label: {
                SharedIcon(name: command["icon"].string, size: CGFloat(panel["tile_icon_size"].number))
                    .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle(selected: option["Action"]["checkable"].bool && command["selected"].bool, corner: .half))
                .disabled(!command["enabled"].bool).opacity(command["enabled"].bool ? 1 : 0.36)
                .accessibilityLabel(command["label"].string).help(command["tooltip"].string)
                .accessibilityIdentifier("toolbar-action-" + command["id"].string)
        }
    }
}

/// Connected icon choices shared by Tool Options and the Tool settings panel.
struct SegmentedChoiceBar: View {
    @Environment(\.editorPalette) private var surface
    let choice: JSON
    let prefix: String
    let height: CGFloat?
    let iconSize: CGFloat
    var shape = SquircleShape.control
    var stacked = false
    let palette: EditorPalette
    let send: (JSON) -> Void
    var body: some View {
        let items = choice["items"].array
        let segments = ForEach(items.indices, id: \.self) { index in
            let item = items[index]
            Button { send(item) } label: {
                SharedIcon(name: item["icon"].string, size: iconSize)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(item["selected"].bool ? surface.active : palette["input"],
                        in: shape.segment(index, of: items.count, stacked: stacked))
                    .contentShape(Rectangle())
            }.buttonStyle(.plain)
                .accessibilityLabel(item["label"].string).help(item["label"].string)
                .accessibilityAddTraits(item["selected"].bool ? .isSelected : [])
                .accessibilityIdentifier("\(prefix)-segment-\(choice["id"].string)-\(index)")
        }
        Group {
            if stacked { VStack(spacing: 0) { segments } } else { HStack(spacing: 0) { segments } }
        }.frame(height: height)
            .accessibilityElement(children: .contain)
            .accessibilityLabel(choice["label"].string).accessibilityIdentifier("\(prefix)-segments-" + choice["id"].string)
    }
}

private struct ToolbarChoiceField: View {
    @ObservedObject var store: EditorStore
    let choice: JSON
    let component: JSON
    let vertical: Bool
    let labeled: Bool
    let stacked: Bool
    let iconSize: CGFloat
    @State private var open = false
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var items: [JSON] { choice["items"].array }
    var body: some View {
        if choice["segmented"].bool {
            SegmentedChoiceBar(choice: choice, prefix: "toolbar", height: vertical ? nil : 24, iconSize: vertical ? iconSize : 16,
                shape: vertical ? SquircleShape.tile : SquircleShape.control, stacked: vertical && stacked, palette: palette) { item in
                store.toolbarEdit(component, item["action"].raw)
            }
        } else {
            let selected = items.first { $0["selected"].bool } ?? items.first ?? JSON()
            Button { open = true } label: {
                HStack(spacing: 6) {
                    SharedIcon(name: selected["icon"].string)
                    if !vertical || labeled { Text(selected["label"].string).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading) }
                    if !vertical { SharedIcon(name: "chevron-down", size: 12) }
                }.padding(.horizontal, vertical && !labeled ? 2 : 8)
                    .frame(maxWidth: .infinity, maxHeight: vertical ? .infinity : 24)
                    .background(vertical ? Color.clear : palette["input"], in: SquircleShape.control)
                    .contentShape(Rectangle())
            }.buttonStyle(.plain)
                .accessibilityLabel(choice["label"].string).accessibilityValue(selected["label"].string)
                .help(choice["label"].string)
                .accessibilityIdentifier("toolbar-choice-" + choice["id"].string)
                .editorPopover(isPresented: $open) {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(items.indices, id: \.self) { index in
                            let item = items[index]
                            Button {
                                open = false; store.toolbarEdit(component, item["action"].raw)
                            } label: {
                                HStack(spacing: 8) {
                                    SharedIcon(name: item["icon"].string)
                                    Text(item["label"].string).frame(maxWidth: .infinity, alignment: .leading)
                                    if item["selected"].bool { SharedIcon(name: "selection-checked", size: 12) }
                                }.padding(.horizontal, 10).frame(minHeight: 32).contentShape(Rectangle())
                            }.buttonStyle(EditorControlButtonStyle(selected: item["selected"].bool))
                                .accessibilityIdentifier("toolbar-choice-\(choice["id"].string)-\(index)")
                        }
                    }.padding(6).frame(width: 220)
                }
        }
    }
}

private struct ToolbarNumberField: View {
    @ObservedObject var store: EditorStore
    let field: JSON
    let component: JSON
    let vertical: Bool
    let labeled: Bool
    let style: String
    let preferences: JSON
    @State private var open = false
    @State private var scrubStart: Double?
    private var id: String { field["id"].string }
    private var control: JSON { field["numeric"] }
    private var value: Double { field["value"].number }
    private func change(_ next: Double, _ completion: @escaping @MainActor (String?) -> Void = { _ in }) {
        store.toolbarEdit(component, ["type": "set_tool_setting", "id": id, "value": next], completion: completion)
    }
    private func resolve(_ operation: [String: Any]) {
        if let result = try? store.resolveNumber(control, value: value, operation: operation) { change(result["value"].number) }
    }
    var body: some View {
        let icon = ToolbarUI.cached(["type": "numeric_info", "id": field["id"].raw, "control": control.raw,
            "compact": true, "units": true])["icon"].string
        if vertical {
            let shown = ToolbarUI.formatted(control, value: value, units: style != "small")
            let bare = ToolbarUI.formatted(control, value: value, units: false)
            Button { open = true } label: {
                Group {
                    if labeled {
                        HStack(spacing: 6) {
                            SharedIcon(name: icon)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(field["label"].string).lineLimit(1)
                                ToolbarFaceValue(text: shown["text"].string, bare: bare["text"].string, small: false)
                            }.frame(maxWidth: .infinity, alignment: .leading)
                        }.padding(.horizontal, 8)
                    } else {
                        VStack(spacing: 0) {
                            SharedIcon(name: icon)
                            ToolbarFaceValue(text: shown["text"].string, bare: bare["text"].string, small: style == "small")
                        }.padding(.horizontal, 2)
                    }
                }.padding(.vertical, style == "small" ? 1 : 3)
                    .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle())
                .modifier(ToolbarScrub(gesture: scrub(fill: shown["fill"].number)))
                .accessibilityLabel(field["label"].string).accessibilityValue(shown["text"].string)
                .accessibilityIdentifier("toolbar-setting-" + id)
                .editorPopover(isPresented: $open, placement: .inward) {
                    NumberControl(store: store, label: field["label"].string, value: value, control: control,
                        identifier: "toolbar-popover-" + id) { next, completion in change(next, completion) }
                        .padding(10).frame(width: 240)
                }
        } else {
            HStack(spacing: 4) {
                Group {
                    if preferences["text"].bool { Text(field["label"].string).lineLimit(1) }
                    else { SharedIcon(name: icon) }
                }.accessibilityLabel(field["label"].string)
                    .onTapGesture(count: 2) { store.toolbarEdit(component, ["type": "reset_tool_setting", "id": id]) }
                NumberControl(store: store, label: field["label"].string, value: value, control: control,
                    identifier: "toolbar-" + id, inline: true,
                    toolbar: NumberControl.Toolbar(slider: preferences["sliders"].bool)) { next, completion in
                    change(next, completion)
                }
            }.accessibilityElement(children: .contain).accessibilityIdentifier("toolbar-setting-" + id)
        }
    }
    private func scrub(fill: Double) -> some Gesture {
        DragGesture(minimumDistance: 8).onChanged { event in
            if scrubStart == nil { scrubStart = fill }
            resolve(["type": "position", "position": min(1, max(0, (scrubStart ?? fill) - event.translation.height / 200))])
        }.onEnded { _ in scrubStart = nil }
    }
}

private struct ToolbarFaceValue: View {
    let text: String
    let bare: String
    let small: Bool
    var body: some View {
        ViewThatFits(in: .horizontal) {
            Text(text).lineLimit(1).fixedSize()
            Text(bare).lineLimit(1).font(small && bare.count >= 4 ? .system(size: 11 * 4 / 3 * 0.9) : nil).fixedSize()
        }.monospacedDigit()
    }
}

private struct ToolbarScrub<G: Gesture>: ViewModifier {
    let gesture: G
    func body(content: Content) -> some View {
        #if os(iOS)
        content.simultaneousGesture(gesture)
        #else
        content
        #endif
    }
}
