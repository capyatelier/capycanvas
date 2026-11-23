import SwiftUI

struct WorkspaceBubbleLayer: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var bubbles: WorkspaceBubbles
    @Environment(\.accessibilityReduceMotion) private var reducedMotion
    var body: some View {
        ZStack(alignment: .topLeading) {
            ForEach(bubbles.items) { item in
                BubbleDab(store: store, item: item).allowsHitTesting(false)
            }
            ForEach(bubbles.items) { item in
                WorkspaceBubble(store: store, item: item, placement: item.placement)
                    .id(ObjectIdentifier(item))
            }
            Color.clear.frame(width: 1, height: 1)
                .editorPopover(isPresented: Binding(get: { !bubbles.menu.isNull }, set: { if !$0 { bubbles.closeMenu() } }),
                    placement: .inward, owner: bubbles.popup) {
                    EditorActionMenu(model: bubbles.contextMenu(),
                        identifier: "bubble-context-menu", dismiss: bubbles.closeMenu)
                }.offset(x: bubbles.menuPoint.x, y: bubbles.menuPoint.y).allowsHitTesting(false)
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .accessibilityElement(children: .contain).accessibilityIdentifier("tool-bubble-desk")
            .onChange(of: reducedMotion, initial: true) { _, value in bubbles.reducedMotion = value }
    }
}

private struct WorkspaceBubble: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var item: BubblePresentation
    @ObservedObject var placement: BubblePlacement
    private var view: JSON { item.view }
    private var half: CGFloat { item.model["reach"].number + 60 }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]).glassy }
    var body: some View {
        ZStack {
            if !item.card.isNull {
                BubbleLayerCard(store: store, item: item)
                    .frame(width: item.card["size"][0].number, height: item.card["size"][1].number)
                    .scaleEffect(0.6 + 0.4 * item.cardProgress).opacity(item.cardProgress)
                    .allowsHitTesting(!placement.ignoresInput && !view["card"].isNull)
            }
            BubbleParts(store: store, item: item, placement: placement, half: half, palette: palette)
                .allowsHitTesting(false)
        }.frame(width: half * 2, height: half * 2)
            .scaleEffect(placement.pinch * (placement.homing ? 0.82 : placement.lifted ? 1.04 : 1))
            .opacity(placement.displayOpacity)
            .position(placement.center)
            .accessibilityElement(children: .contain).accessibilityIdentifier("tool-bubble-\(item.id)")
            .environment(\.editorPalette, palette)
    }
}

private struct BubbleDab: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var item: BubblePresentation
    @ObservedObject private var placement: BubblePlacement
    init(store: EditorStore, item: BubblePresentation) { self.store = store; self.item = item; placement = item.placement }
    private func color(_ source: JSON) -> Color {
        guard store.colorPanel["hdr"].bool else { return ColorUI.preview(source)["rgba"].paintColor }
        let viewing = store.colorViewing
        let linear = ColorUI.resolve(["type": "hdr_preview", "color": source.raw,
            "document_space": viewing["document_space"].raw, "recipe": viewing["recipe"].raw, "headroom": viewing["headroom"].number])["linear"]
        return Color(.sRGBLinear, red: linear[0].number, green: linear[1].number, blue: linear[2].number, opacity: linear[3].number)
    }
    var body: some View {
        if let stamp = item.stamp, !item.view["stamp"].isNull {
            let source = item.view["stamp"], size = source["size"]
            let center = source["center"].point
            Image(decorative: stamp, scale: 1).resizable().renderingMode(.template)
                .foregroundStyle(color(source["color"]))
                .frame(width: size[0].number * item.stampExtent, height: size[1].number * item.stampExtent).position(x: placement.center.x + center.x, y: placement.center.y + center.y)
                .opacity(placement.opacity)
        }
    }
}

private struct BubbleParts: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var item: BubblePresentation
    @ObservedObject var placement: BubblePlacement
    let half: CGFloat
    let palette: EditorPalette
    private var view: JSON { item.view }
    private var opacity: Double { placement.displayOpacity }
    private func point(_ value: JSON) -> CGPoint { CGPoint(x: half + value[0].number, y: half + value[1].number) }
    private func highlighted(_ hit: JSON) -> Double {
        item.pressed.stableKey == hit.stableKey ? 0.14 : item.hover.stableKey == hit.stableKey ? 0.07 : 0
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            if view["kind"].string == "color" { wheel }
            else if item.card.isNull { face.opacity(item.faceProgress).scaleEffect(0.6 + 0.4 * item.faceProgress) }
            if view["kind"].string == "medium" {
                ForEach(view["dots"].array.indices, id: \.self) { index in
                    Circle().fill(palette["text"].opacity(view["dots"][index].bool ? 0.9 : 0.3)).frame(width: 3.5, height: 3.5)
                        .position(x: half + item.model["dots"][index][0].number, y: half + item.model["dots"][index][1].number)
                }
            }
            ZStack {
                ForEach(item.rings.array.indices, id: \.self) { index in
                    BubbleHalo(ring: item.rings[index], geometry: item.ringGeometry[index], half: half,
                        palette: palette, strength: opacity * item.haloProgress)
                }
            }.frame(width: half * 2, height: half * 2)
                .scaleEffect(0.85 + 0.15 * item.haloProgress).opacity(item.haloProgress)
            ForEach(view["hints"].array.indices, id: \.self) { index in
                let hint = view["hints"][index]
                Circle().fill(palette["text"].opacity(0.35 * hint[1].number)).frame(width: 5, height: 5).position(point(hint[0]))
            }
            ForEach(item.beads.filter { $0["placement"]["bead"]["kind"].string != "palettes" }, id: \.bubbleBeadKey) { bead($0) }
            ForEach(item.beads.filter { $0["placement"]["bead"]["kind"].string == "palettes" }, id: \.bubbleBeadKey) { bead($0) }
            if !view["pill"].isNull { pill }
            ForEach(item.choices) { chooser($0) }
            if !view["readout"].isNull {
                BubbleTextPlate(text: view["readout"].string, palette: palette, strength: opacity, lifted: placement.lifted)
                    .monospacedDigit().position(point(item.model["readout"]))
            }
        }.frame(width: half * 2, height: half * 2)
    }
    private var wheel: some View {
        let side = view["diameter"].number, wheel = side - 14
        let color = item.model["color"], bounds = color["bounds"].rect
        return ZStack(alignment: .topLeading) {
            if color.isNull {
                BubblePlate(size: CGSize(width: side, height: side), palette: palette, strength: opacity, lifted: placement.lifted)
                    .position(x: half, y: half)
            } else {
                let shape = BubbleColorShape(color: color)
                shape.fill(palette.glassPanel)
                    .background { OutsideShadow(shape: shape, opacity: placement.lifted ? 0.16 : 0.08,
                        radius: (placement.lifted ? 12 : 8) / 3, y: placement.lifted ? 4.5 : 2) }
                    .frame(width: bounds.width, height: bounds.height)
                    .modifier(GlassColorBubbleRegistration(color: color, strength: opacity))
                    .position(x: half + bounds.midX, y: half + bounds.midY)
            }
            ZStack {
                ColorWheelDrawing(model: store.colorPanel, bounds: JSON([0, 0, wheel, wheel]), viewing: store.colorPanel["hdr"].bool ? store.colorViewing : JSON())
                    .frame(width: wheel, height: wheel)
            }.frame(width: side, height: side)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(view["label"].string).accessibilityIdentifier("bubble-wheel-\(item.id)")
                .position(x: half, y: half)
            if !color.isNull { BubbleIntensity(store: store, item: item, data: color["intensity"], half: half) }
        }.frame(width: half * 2, height: half * 2)
    }
    @ViewBuilder private var face: some View {
        let size = CGSize(width: view["face"][0].number, height: view["face"][1].number)
        let layers = view["kind"].string == "layers", history = view["kind"].string == "history"
        ZStack {
            BubblePlate(size: size, palette: palette, strength: opacity * item.faceProgress * (view["enabled"].bool ? 1 : 0.45),
                selected: view["selected"].bool, highlight: history ? 0 : highlighted(JSON("face")),
                radius: layers ? 20 : nil, shadow: !layers, lifted: placement.lifted,
                alpha: view["enabled"].bool ? 1 : 0.45)
            if history {
                ForEach(view["buttons"].array.indices, id: \.self) { index in
                    let button = view["buttons"][index], hit = JSON(["half": index])
                    ZStack {
                        Circle().fill(palette["text"].opacity(highlighted(hit)))
                        SharedIcon(name: button["icon"].string, size: 20).opacity(button["enabled"].bool ? 1 : 0.35)
                    }.frame(width: size.height, height: size.height)
                        .position(x: size.width / 2 + item.model["history"][index][0].number, y: size.height / 2)
                        .accessibilityElement().accessibilityLabel(button["label"].string)
                        .accessibilityAction { tap(item.model["history"][index].point) }
                        .accessibilityIdentifier("bubble-history-\(index)")
                }
            } else if layers {
                BubbleLayerFace(store: store, item: item)
            } else {
                PanelIcon(name: view["icon"].string, size: 20)
                    .offset(y: !view["preview"].isNull ? -6 : view["kind"].string == "medium" ? -4 : 0)
                    .opacity(view["enabled"].bool ? 1 : 0.45)
                if view["variants"].bool {
                    ToolGroupMarker().frame(width: 16, height: 16).offset(x: size.width / 2 - 17, y: size.height / 2 - 17)
                        .opacity(view["enabled"].bool ? 1 : 0.45)
                }
                if !view["preview"].isNull {
                    BundledPreview(name: "preview-\(view["preview"].uint)-\(store.state["theme"].string)")
                        .frame(width: 32, height: 8).opacity(0.78).offset(y: 11)
                }
            }
        }.frame(width: size.width, height: size.height).position(x: half, y: half)
            .accessibilityElement(children: history ? .contain : .ignore).accessibilityLabel(view["label"].string)
            .accessibilityAction { tap(.zero) }
            .accessibilityAddTraits(.isButton)
            .accessibilityAddTraits(view["selected"].bool ? .isSelected : [])
            .accessibilityIdentifier("bubble-face-\(item.id)")
    }
    private func tap(_ point: CGPoint) { store.bubbles.action(["type": "tap", "id": item.id, "position": [point.x, point.y]]) }
    private func chooser(_ transition: BubblePresentation.Choice) -> some View {
        let choice = transition.value, brush = choice["kind"].string == "brush"
        let size = CGSize(width: brush ? 152 : 44, height: brush ? 48 : 44)
        let hit = JSON(["item": transition.index]), active = !transition.leaving && item.pressed.stableKey == hit.stableKey
        let hot = !transition.leaving && (active || item.hover.stableKey == hit.stableKey)
        let alpha = transition.alpha, enabled = choice["enabled"].bool ? 1.0 : 0.45
        return ZStack(alignment: .topLeading) {
            ZStack {
                BubblePlate(size: size, palette: palette, strength: opacity * enabled * alpha,
                    selected: choice["selected"].bool, highlight: transition.leaving ? 0 : highlighted(hit),
                    lifted: placement.lifted, alpha: enabled)
                if brush {
                    Text(choice["label"].string).lineLimit(1).padding(.horizontal, 8).padding(.bottom, 5)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom).opacity(0.8 * enabled)
                    if !choice["preview"].isNull {
                        BundledPreview(name: "preview-\(choice["preview"].uint)-\(store.state["theme"].string)")
                            .frame(width: size.width * 0.68, height: 14).offset(y: -11)
                    } else { SharedIcon(name: choice["icon"].string, size: 18).offset(y: -8).opacity(enabled) }
                } else { SharedIcon(name: choice["icon"].string, size: 18).opacity(enabled) }
            }.frame(width: size.width, height: size.height)
                .scaleEffect((0.6 + 0.4 * alpha) * (hot ? brush ? 1.06 : 1.12 : 1)).opacity(alpha)
                .position(point(choice["center"]))
                .accessibilityElement().accessibilityLabel(choice["label"].string)
                .accessibilityAction {
                    guard !transition.leaving else { return }
                    store.bubbles.action(["type": "choose", "id": item.id, "phase": "down", "position": choice["center"].raw])
                    store.bubbles.action(["type": "choose", "id": item.id, "phase": "up", "position": choice["center"].raw])
                }
                .accessibilityIdentifier("bubble-choice-\(item.id)-\(transition.index)")
                .accessibilityAddTraits(.isButton)
            if !brush && !transition.leaving && alpha == 1 && (choice["shown"].bool || hot) {
                let width = min(96, EditorTextMetrics.width(choice["label"].string, size: store.catalog["text_size_pt"].number * 4 / 3, weight: .regular) + 18)
                let tag = BubbleUI.resolve(["type": "chooser_tag", "center": choice["center"].raw, "width": width])
                BubbleTextPlate(text: choice["label"].string, palette: palette, strength: opacity, width: width,
                    horizontal: 9, vertical: 3, radius: 9, lifted: placement.lifted)
                    .position(x: half + tag["x"].number + tag["width"].number / 2, y: half + tag["y"].number + tag["height"].number / 2)
            }
        }.frame(width: half * 2, height: half * 2)
    }
    private func bead(_ bead: JSON) -> some View {
        let placement = bead["placement"], side = placement["size"].number
        let kind = placement["bead"]["kind"].string, palettes = kind == "palettes", alpha = palettes ? 1 : placement["alpha"].number
        return ZStack {
            if palettes {
                BubblePlate(size: CGSize(width: side, height: side), palette: palette, strength: opacity,
                    selected: bead["selected"].bool, lifted: self.placement.lifted)
                SharedIcon(name: "palette", size: 13)
            } else {
                Circle().fill(palette["panel"]).opacity(alpha)
                Group {
                    if store.colorPanel["hdr"].bool { HDRColorSwatch(color: bead["color"], viewing: store.colorViewing).clipShape(Circle()) }
                    else { ColorPaintPreview(rgba: ColorUI.preview(bead["color"])["rgba"]) }
                }.padding(kind == "foreground" ? 3 : 1)
                    .overlay(Circle().strokeBorder(palette["text"].opacity(bead["selected"].bool ? 1 : 0.25), lineWidth: bead["selected"].bool ? 2 : 1))
                    .opacity(alpha)
            }
        }.frame(width: side, height: side)
            .background {
                if !palettes { BubbleShadow(size: CGSize(width: side, height: side), spread: 3, drop: 1, opacity: 0.13 * alpha) }
            }.position(point(placement["center"]))
            .accessibilityElement().accessibilityLabel(bead["label"].string)
            .accessibilityIdentifier("bubble-bead-\(item.id)-\(kind)-\(placement["bead"]["index"].uint)")
            .accessibilityAction {
                store.bubbles.action(["type": "beads", "id": item.id, "phase": "down", "position": placement["center"].raw])
                store.bubbles.action(["type": "beads", "id": item.id, "phase": "up", "position": placement["center"].raw])
            }
    }
    private var pill: some View {
        let bounds = item.model["pill"].rect
        return ZStack {
            BubblePlate(size: bounds.size, palette: palette, strength: opacity, lifted: placement.lifted)
            HStack(spacing: 2) {
                SharedIcon(name: "go-previous", size: 16).frame(width: 32)
                Text(view["pill"].string).bold().lineLimit(1).frame(maxWidth: .infinity)
                SharedIcon(name: "go-next", size: 16).frame(width: 32)
            }
        }.frame(width: bounds.width, height: bounds.height).position(x: half + bounds.midX, y: half + bounds.midY)
    }
}

private struct BubbleColorShape: Shape {
    let color: JSON
    func path(in rect: CGRect) -> Path {
        let bounds = color["bounds"].rect, scale = rect.width / max(1, bounds.width)
        return Path { path in
            for arc in color["outline"].array {
                let center = arc["center"].point
                path.addArc(center: CGPoint(x: rect.minX + (center.x - bounds.minX) * scale,
                    y: rect.minY + (center.y - bounds.minY) * scale), radius: arc["radius"].number * scale,
                    startAngle: .radians(arc["start"].number), endAngle: .radians(arc["start"].number + arc["sweep"].number),
                    clockwise: arc["sweep"].number < 0)
            }
            path.closeSubpath()
        }
    }
}

private struct BubbleIntensity: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var item: BubblePresentation
    let data: JSON
    let half: CGFloat
    @Environment(\.editorPalette) private var palette
    #if os(macOS)
    @FocusState private var focused: Bool
    #endif
    @StateObject private var resources = ColorUIResourceCache()
    private var bounds: CGRect {
        let points = data["path"].array.map(\.point)
        let margin = max(data["geometry"]["width"].number / 2 + 5, data["geometry"]["marker_radius"].number + 2)
        let xs = points.map(\.x), ys = points.map(\.y)
        return CGRect(x: xs.min() ?? 0, y: ys.min() ?? 0,
            width: (xs.max() ?? 0) - (xs.min() ?? 0), height: (ys.max() ?? 0) - (ys.min() ?? 0)).insetBy(dx: -margin, dy: -margin)
    }
    private func point(_ value: JSON, in bounds: CGRect) -> CGPoint { CGPoint(x: value[0].number - bounds.minX, y: value[1].number - bounds.minY) }
    private func set(_ value: Double) {
        store.bubbles.input.setIntensity(item.id, stops: value)
    }
    var body: some View {
        let bounds = bounds
        let model = store.colorPanel, viewing = store.colorViewing
        let arc = resources.resolve(["type": "intensity_arc", "size": item.view["color"]["stage"].number, "stops": data["stops"].number,
            "base": model["base"].raw, "document_space": viewing["document_space"].raw, "depth": model["document_depth"].raw,
            "recipe": viewing["recipe"].raw, "headroom": viewing["headroom"].number])
        HDRIntensityTrack(points: data["path"].array.map { point($0, in: bounds) }, colors: arc["colors"].array.map(HDRIntensityTrack.linearColor),
            center: point(data["geometry"]["center"], in: bounds), marker: point(data["point"], in: bounds), width: data["geometry"]["width"].number,
            radius: data["geometry"]["marker_radius"].number, markerFill: HDRIntensityTrack.linearColor(arc["marker_color"]), rimOpacity: 0.5,
            zeroTick: data["zero"].array.map { point($0, in: bounds) }, ink: palette["text"])
            .frame(width: bounds.width, height: bounds.height)
            .disabled(!data["enabled"].bool)
            #if os(macOS)
            .focusable(data["enabled"].bool).focused($focused).focusEffectDisabled()
            .onChange(of: item.pressed.stableKey) { _, _ in if item.pressed.bubbleHit == "intensity" { focused = true } }
            .onKeyPress(keys: [.leftArrow, .downArrow, .rightArrow, .upArrow, .home]) { press in
                let key = press.key == .home ? "home" : press.key == .leftArrow ? "arrowleft"
                    : press.key == .downArrow ? "arrowdown" : press.key == .rightArrow ? "arrowright" : "arrowup"
                return store.bubbles.input.intensityKey(key, id: item.id,
                    command: !press.modifiers.intersection([.command, .control]).isEmpty,
                    alt: press.modifiers.contains(.option)) ? .handled : .ignored
            }
            #endif
            .accessibilityRepresentation {
                Slider(value: Binding(get: { data["stops"].number }, set: set),
                    in: data["range"][0].number...data["range"][1].number, step: 0.1) { Text(data["label"].string) }
                    .accessibilityValue(Text(data["text"].string))
                    .accessibilityIdentifier("bubble-intensity-\(item.id)")
                    .accessibilityAdjustableAction { set(data["stops"].number + ($0 == .increment ? 0.1 : -0.1)) }
                    .disabled(!data["enabled"].bool)
            }
            .position(x: half + bounds.midX, y: half + bounds.midY)
    }
}

private struct BubbleHalo: View {
    let ring: JSON
    let geometry: JSON
    let half: CGFloat
    let palette: EditorPalette
    let strength: Double
    private func arc(_ start: Double, _ sweep: Double) -> Path {
        Path { path in path.addArc(center: CGPoint(x: half, y: half), radius: ring["arc"]["radius"].number,
            startAngle: .degrees(start), endAngle: .degrees(start + sweep), clockwise: sweep < 0) }
    }
    var body: some View {
        let a = ring["arc"], start = a["start"].number, sweep = a["sweep"].number
        let extra = 21 / a["radius"].number * 180 / .pi, direction = sweep < 0 ? -1.0 : 1.0
        ZStack(alignment: .topLeading) {
            arc(start - direction * extra, sweep + direction * extra).stroke(palette.glassPanel, style: StrokeStyle(lineWidth: a["band"].number, lineCap: .round))
            arc(start, sweep).stroke(palette["text"].opacity(0.18), style: StrokeStyle(lineWidth: 4, lineCap: .round))
            if ring["fill"].number > 0.004 {
                arc(start, sweep * ring["fill"].number).stroke(palette["text"].opacity(0.5), style: StrokeStyle(lineWidth: 4, lineCap: .round))
            }
            Circle().fill(palette["thumb"]).overlay(Circle().strokeBorder(palette["text"].opacity(0.6), lineWidth: 1))
                .frame(width: ring["active"].bool ? 15 : 14, height: ring["active"].bool ? 15 : 14)
                .position(x: half + geometry["thumb"][0].number, y: half + geometry["thumb"][1].number)
            SharedIcon(name: ring["icon"].string, size: 16)
                .position(x: half + geometry["icon"][0].number, y: half + geometry["icon"][1].number)
        }.frame(width: half * 2, height: half * 2).opacity(ring["enabled"].bool ? 1 : 0.4)
            .modifier(GlassArcRegistration(arc: a, strength: strength * (ring["enabled"].bool ? 1 : 0.4)))
            .accessibilityElement().accessibilityLabel(ring["label"].string).accessibilityValue(ring["text"].string)
    }
}

private struct BubblePlate: View {
    let size: CGSize
    let palette: EditorPalette
    let strength: Double
    var selected = false
    var highlight = 0.0
    var radius: CGFloat?
    var rounded = false
    var shadow = true
    var lifted = false
    var alpha = 1.0
    var body: some View {
        let corner = radius ?? min(size.width, size.height) / 2
        let shape = SquircleShape(corner)
        ZStack {
            Group {
                if radius == nil {
                    Capsule().fill(palette.glassPanel)
                    if selected { Capsule().fill(palette.active) }
                    if highlight > 0 { Capsule().fill(palette["text"].opacity(highlight)) }
                } else if rounded {
                    RoundedRectangle(cornerRadius: corner).fill(palette.glassPanel)
                } else {
                    shape.fill(palette.glassPanel)
                    if selected { shape.fill(palette.active) }
                    if highlight > 0 { shape.fill(palette["text"].opacity(highlight)) }
                }
            }.opacity(alpha)
        }.frame(width: size.width, height: size.height)
            .background {
                if shadow { BubbleShadow(size: size, radius: rounded ? min(size.width, size.height) / 2 : corner, spread: lifted ? 12 : 8, drop: lifted ? 4.5 : 2, opacity: lifted ? 0.16 : 0.08) }
            }.modifier(GlassRegistration(shape: shape, circular: radius == nil, rounded: rounded, strength: strength))
    }
}

private struct BubbleShadow: View {
    let size: CGSize
    var radius: CGFloat = .greatestFiniteMagnitude
    let spread: CGFloat
    let drop: CGFloat
    let opacity: Double
    var body: some View {
        Canvas { context, canvas in
            let rect = CGRect(x: spread, y: spread, width: size.width, height: size.height)
            let radius = min(radius, size.width / 2, size.height / 2), reach = radius + spread
            context.clip(to: Path(roundedRect: rect, cornerRadius: radius), options: .inverse)
            let left = rect.minX + radius, right = rect.maxX - radius, top = rect.minY + radius + drop, bottom = rect.maxY - radius + drop
            let stops = (0...4).map { index -> Gradient.Stop in
                let t = Double(index) / 4
                return .init(color: .black.opacity(opacity * (1 - t) * (1 - t)), location: (radius + spread * t) / reach)
            }
            let gradient = Gradient(stops: [.init(color: .black.opacity(opacity), location: 0)] + stops)
            for (x, y, clip) in [
                (left, top, CGRect(x: left - reach, y: top - reach, width: reach, height: reach)),
                (right, top, CGRect(x: right, y: top - reach, width: reach, height: reach)),
                (left, bottom, CGRect(x: left - reach, y: bottom, width: reach, height: reach)),
                (right, bottom, CGRect(x: right, y: bottom, width: reach, height: reach))
            ] {
                var corner = context; corner.clip(to: Path(clip))
                corner.fill(Path(CGRect(origin: .zero, size: canvas)), with: .radialGradient(gradient, center: CGPoint(x: x, y: y), startRadius: 0, endRadius: reach))
            }
            func band(_ length: CGFloat) -> Gradient {
                let edge = spread / (length + spread * 2)
                let start = (0...4).map { index -> Gradient.Stop in
                    let t = Double(index) / 4
                    return .init(color: .black.opacity(opacity * t * t), location: edge * t)
                }
                let end = (0...4).map { index -> Gradient.Stop in
                    let t = Double(index) / 4
                    return .init(color: .black.opacity(opacity * (1 - t) * (1 - t)), location: 1 - edge + edge * t)
                }
                return Gradient(stops: start + end)
            }
            if right > left {
                let area = CGRect(x: left, y: rect.minY + drop - spread, width: right - left, height: size.height + spread * 2)
                context.fill(Path(area), with: .linearGradient(band(size.height), startPoint: CGPoint(x: left, y: area.minY), endPoint: CGPoint(x: left, y: area.maxY)))
            }
            if bottom > top {
                let area = CGRect(x: rect.minX - spread, y: top, width: size.width + spread * 2, height: bottom - top)
                context.fill(Path(area), with: .linearGradient(band(size.width), startPoint: CGPoint(x: area.minX, y: top), endPoint: CGPoint(x: area.maxX, y: top)))
            }
        }.frame(width: size.width + spread * 2, height: size.height + spread * 2 + drop)
            .offset(y: drop / 2).allowsHitTesting(false)
    }
}

private struct BubbleTextPlate: View {
    let text: String
    let palette: EditorPalette
    let strength: Double
    var width: CGFloat?
    var horizontal: CGFloat = 7
    var vertical: CGFloat = 2
    var radius: CGFloat?
    var height: CGFloat?
    var lifted = false
    var body: some View {
        Text(text).lineLimit(1).padding(.horizontal, horizontal).padding(.vertical, vertical).frame(width: width, height: height)
            .background { GeometryReader { geometry in BubblePlate(size: geometry.size, palette: palette, strength: strength, radius: radius, rounded: radius != nil, lifted: lifted) } }
            .fixedSize(horizontal: width == nil, vertical: true)
    }
}

private struct BubbleLayerCard: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var item: BubblePresentation
    @ObservedObject private var placement: BubblePlacement
    @StateObject private var inspector: BubbleLayerInspector
    init(store: EditorStore, item: BubblePresentation) {
        self.store = store; self.item = item; placement = item.placement
        _inspector = StateObject(wrappedValue: BubbleLayerInspector(store: store, item: item))
    }
    var body: some View {
        let card = item.card, size = CGSize(width: card["size"][0].number, height: card["size"][1].number)
        let palette = EditorPalette(source: store.state["palette"])
        LayerInspector(store: store, drawer: inspector, width: size.width, height: size.height)
            .clipShape(SquircleShape.surface.fittedClip)
            .background { BubblePlate(size: size, palette: palette.glassy, strength: placement.displayOpacity * item.cardProgress, radius: 18, lifted: placement.lifted) }
            .environment(\.bubbleEdits, store.bubbles.input)
            .environment(\.bubbleCardID, item.id)
            .environment(\.editorPalette, palette.glassy)
            .onChange(of: card.stableKey, initial: true) { _, _ in inspector.receive(card) }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(item.view["label"].string)
            .accessibilityIdentifier("bubble-layers-card-\(item.id)")
    }
}

private struct BubbleLayerFace: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var item: BubblePresentation
    @ObservedObject private var placement: BubblePlacement
    @ObservedObject private var thumbnails: LayerThumbnails
    @State private var token = UUID().uuidString
    init(store: EditorStore, item: BubblePresentation) { self.store = store; self.item = item; placement = item.placement; thumbnails = store.layerThumbnails }
    var body: some View {
        ZStack {
            ZStack {
                Color.white
                if let image = thumbnails.images[LayerThumbnails.key(item.view["layer"].uint, false)] {
                    Image(decorative: image, scale: 1).resizable().scaledToFill()
                }
            }.frame(width: 46, height: 46).clipShape(SquircleShape(13))
                .overlay { SquircleShape(14).strokeBorder(Color.gray.opacity(0.28), lineWidth: 1).frame(width: 48, height: 48) }
                .opacity(item.view["enabled"].bool ? 1 : 0.45)
            let text = String(item.view["layers"].uint)
            let badge = BubbleUI.resolve(["type": "count_badge", "text": [EditorTextMetrics.width(text, size: store.catalog["text_size_pt"].number * 4 / 3, weight: .semibold), 22]])
            BubbleTextPlate(text: text, palette: EditorPalette(source: store.state["palette"]).glassy,
                strength: placement.displayOpacity * item.faceProgress, width: badge[1][0].number, horizontal: 0, vertical: 0, height: 22, lifted: placement.lifted)
                .fontWeight(.semibold).offset(x: badge[0][0].number, y: badge[0][1].number)
        }.onChange(of: item.view["layer"].uint, initial: true) { _, layer in thumbnails.show(token: token, id: layer) }
            .onDisappear { thumbnails.hide(token) }
    }
}
