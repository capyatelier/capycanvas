import SwiftUI

/// One atomic interval: two value readouts around a track with facing handles.
/// Shared Rust resolves positions, steps and typed values; the host freezes the
/// visible domain while a contact is live and restores the endpoint on cancel.
struct RangeControl: View {
    @ObservedObject var store: EditorStore
    let bounds: [JSON]
    let label: String
    let prefix: String
    var showSlider = true
    let change: (Int, Double, @escaping @MainActor (String?) -> Void) -> Void
    @State private var values: [Double] = []
    @State private var contact: Contact?
    @FocusState private var focus: Int?
    @GestureState private var tracking = false
    private struct Contact { let index: Int; let before: Double; let domain: [Double]; let offset: CGFloat }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var published: [Double] { bounds.map { $0["value"].number } }
    private var current: [Double] { values.count == 2 ? values : published }
    private var idleDomain: [Double] {
        [min(bounds[0]["numeric"]["soft_min"].number, current[0]), max(bounds[1]["numeric"]["soft_max"].number, current[1])]
    }
    var body: some View {
        HStack(spacing: 4) {
            endpoint(0)
            if showSlider { track }
            endpoint(1)
        }.frame(height: 28)
            .help(label)
            .accessibilityElement(children: .contain).accessibilityIdentifier(prefix + "-range-tonal")
            .onAppear { values = published }
            .onChange(of: published) { _, next in if contact == nil { values = next } }
            .onDisappear { contact = nil }
    }
    private func endpoint(_ index: Int) -> some View {
        let field = bounds[index]
        return NumberControl(store: store, label: field["label"].string + " — " + label, value: current[index],
            control: field["numeric"], identifier: prefix + "-" + field["id"].string, valueOnly: true) { next, completion in
            contact = nil
            change(index, next, completion)
        }.fixedSize()
    }
    private var track: some View {
        GeometryReader { geometry in
            let width = geometry.size.width, domain = contact?.domain ?? idleDomain
            let x = current.map { position($0, domain, width) }
            ZStack(alignment: .topLeading) {
                Canvas { context, size in draw(context, size, x) }
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .local)
                        .updating($tracking) { _, state, _ in state = true }
                        .onChanged { event in
                            if contact == nil { press(event.location.x, width) } else { move(event.location.x, width) }
                        }
                        .onEnded { _ in contact = nil })
                ForEach(0..<2, id: \.self) { index in handle(index, x: x[index], height: geometry.size.height) }
            }
        }.frame(minWidth: 64, maxWidth: .infinity, maxHeight: .infinity)
            .onChange(of: tracking) { _, active in if !active { cancel() } }
            .accessibilityElement(children: .contain).accessibilityIdentifier(prefix + "-range-track")
    }
    private func handle(_ index: Int, x: CGFloat, height: CGFloat) -> some View {
        let field = bounds[index]
        return Color.clear.frame(width: 16, height: height).contentShape(Rectangle())
            .focusable().focused($focus, equals: index).focusEffectDisabled()
            .onKeyPress(keys: [.leftArrow, .downArrow, .rightArrow, .upArrow, .pageUp, .pageDown, .home, .end, .escape]) { press in
                key(index, press.key)
            }
            .offset(x: x - 8)
            .allowsHitTesting(false)
            .accessibilityElement()
            .accessibilityLabel(field["label"].string + " — " + label)
            .accessibilityValue(formatted(index))
            .accessibilityAdjustableAction { direction in
                step(index, direction == .increment ? 1 : -1)
            }
            .accessibilityIdentifier(prefix + "-range-handle-" + field["id"].string)
    }
    private func draw(_ context: GraphicsContext, _ size: CGSize, _ x: [CGFloat]) {
        let text = palette["text"], center = size.height / 2
        context.fill(Path(CGRect(x: 8, y: center - 2, width: max(0, size.width - 16), height: 4)), with: .color(text.opacity(0.2)))
        context.fill(Path(CGRect(x: x[0], y: center - 2, width: max(0, x[1] - x[0]), height: 4)), with: .color(text.opacity(0.65)))
        for (index, origin) in [x[0] - 7, x[1] + 1].enumerated() {
            context.fill(Path(roundedRect: CGRect(x: origin, y: center - 7, width: 6, height: 14), cornerRadius: 2), with: .color(text))
            if focus == index {
                context.stroke(Path(roundedRect: CGRect(x: origin - 3, y: center - 10, width: 12, height: 20), cornerRadius: 4),
                    with: .color(text), lineWidth: 1)
            }
        }
    }
    private func position(_ value: Double, _ domain: [Double], _ width: CGFloat) -> CGFloat {
        8 + CGFloat((value - domain[0]) / max(1e-9, domain[1] - domain[0])) * max(1, width - 16)
    }
    private func press(_ x: CGFloat, _ width: CGFloat) {
        let domain = idleDomain, p = current.map { position($0, domain, width) }
        let index = abs(p[1] - p[0]) < 1 ? (x >= p[0] ? 1 : 0) : (abs(x - p[0]) <= abs(x - p[1]) ? 0 : 1)
        contact = Contact(index: index, before: current[index], domain: domain, offset: abs(x - p[index]) <= 12 ? x - p[index] : 0)
        focus = index
        move(x, width)
    }
    private func move(_ x: CGFloat, _ width: CGFloat) {
        guard let contact else { return }
        var spec = bounds[contact.index]["numeric"].object
        spec["soft_min"] = contact.domain[0]; spec["soft_max"] = contact.domain[1]
        guard let resolved = try? store.resolveNumber(JSON(spec), value: contact.before,
            operation: ["type": "position", "position": Double(min(1, max(0, (x - contact.offset - 8) / max(1, width - 16))))]) else { return }
        set(contact.index, resolved["value"].number)
    }
    private func cancel() {
        guard let contact else { return }
        self.contact = nil
        set(contact.index, contact.before)
    }
    private func set(_ index: Int, _ value: Double) {
        var next = current
        next[index] = index == 0 ? min(value, next[1]) : max(value, next[0])
        guard next[index] != current[index] else { return }
        values = next
        change(index, next[index]) { _ in }
    }
    private func step(_ index: Int, _ steps: Int) {
        guard let resolved = try? store.resolveNumber(bounds[index]["numeric"], value: current[index],
            operation: ["type": "step", "steps": steps]) else { return }
        set(index, resolved["value"].number)
    }
    private func key(_ index: Int, _ key: KeyEquivalent) -> KeyPress.Result {
        switch key {
        case .escape:
            guard contact != nil else { return .ignored }
            cancel()
        case .leftArrow, .downArrow: step(index, -1)
        case .rightArrow, .upArrow: step(index, 1)
        case .pageDown: step(index, -10)
        case .pageUp: step(index, 10)
        case .home: set(index, index == 0 ? bounds[0]["numeric"]["min"].number : current[0])
        case .end: set(index, index == 0 ? current[1] : bounds[1]["numeric"]["max"].number)
        default: return .ignored
        }
        return .handled
    }
    private func formatted(_ index: Int) -> String {
        (try? store.resolveNumber(bounds[index]["numeric"], value: current[index], operation: ["type": "format"]))?["text"].string ?? ""
    }
}
