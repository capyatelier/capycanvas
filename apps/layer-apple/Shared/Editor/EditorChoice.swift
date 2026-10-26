import SwiftUI

struct PropertyChoiceRow: Layout {
    var labelWidth: CGFloat? = nil
    var labelFraction: CGFloat = 0.45
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let natural = subviews.reduce(CGFloat(4)) { $0 + $1.sizeThatFits(.unspecified).width }
        let width = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? natural
        guard subviews.count == 2 else { return CGSize(width: width, height: 24) }
        let label = min(width * labelFraction, labelWidth ?? subviews[0].sizeThatFits(.unspecified).width)
        return CGSize(width: width, height: max(24,
            subviews[0].sizeThatFits(ProposedViewSize(width: label, height: nil)).height,
            subviews[1].sizeThatFits(ProposedViewSize(width: max(0, width - label - 4), height: nil)).height))
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let label = min(bounds.width * labelFraction, labelWidth ?? subviews[0].sizeThatFits(.unspecified).width)
        subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.midY), anchor: .leading,
            proposal: ProposedViewSize(width: label, height: bounds.height))
        subviews[1].place(at: CGPoint(x: bounds.minX + label + 4, y: bounds.midY), anchor: .leading,
            proposal: ProposedViewSize(width: max(0, bounds.width - label - 4), height: bounds.height))
    }
}

struct EditorChoice: View {
    enum Presentation { case select, layerMenu }
    let label: String
    let options: [String]
    let selected: Int
    let identifier: String
    let background: Color
    var ellipsis = false
    var bold = false
    var presentation: Presentation = .select
    let height: CGFloat
    var menu: ((@escaping @MainActor (AppleContextMenu) -> Void) -> Void)? = nil
    let select: (Int) -> Void
    @State private var choosing = false
    @State private var model: AppleContextMenu?
    @Environment(\.isEnabled) private var enabled
    @Environment(\.compactPanelControls) private var compact
    var body: some View {
        Button { if let menu { menu { model = $0; choosing = true } } else { choosing = true } } label: {
            ChoiceLabelLayout(ellipsis: ellipsis || presentation == .layerMenu, presentation: presentation) {
                ForEach(options.indices, id: \.self) { Text(options[$0]).hidden().accessibilityHidden(true) }
                Text(options.indices.contains(selected) ? options[selected] : label).lineLimit(1)
            }.fontWeight(bold || compact || presentation == .layerMenu ? .bold : .regular).clipped()
                .modifier(ChoiceMeasurement(id: identifier + ":text"))
                .padding(.leading, presentation == .select ? 10 : 6).padding(.trailing, presentation == .select ? 18 : 22)
                .frame(height: height)
                .overlay(alignment: .trailing) {
                    if presentation == .layerMenu {
                        SharedIcon(name: "chevron-down", size: 12)
                            .modifier(ChoiceMeasurement(id: identifier + ":arrow"))
                            .padding(.trailing, 6)
                    } else {
                        ChoiceArrow().stroke(style: StrokeStyle(lineWidth: 2, lineCap: .butt, lineJoin: .miter))
                            .frame(width: 16, height: 16)
                            .modifier(ChoiceMeasurement(id: identifier + ":arrow"))
                    }
                }
                .background(background, in: SquircleShape.control).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle()).opacity(enabled || presentation != .layerMenu ? 1 : 0.36)
            .modifier(ChoiceMeasurement(id: identifier))
            .accessibilityLabel(label).accessibilityValue(options.indices.contains(selected) ? options[selected] : "")
            .accessibilityIdentifier(identifier)
            .editorPopover(isPresented: $choosing) {
                EditorActionMenu(model: model ?? AppleContextMenu(JSON(["sections": [options.indices.map { index in
                    ["label": options[index], "enabled": true, "selected": index == selected,
                     "identifier": identifier + "-option-\(index)", "action": ["index": index]]
                }]])) { select(Int($0["index"].uint)) }, width: 230) { choosing = false; model = nil }
            }
    }
}

/// The web uses the native select disclosure (8×4, a two-point mitered stroke).
/// Its complete 16-point allocation sits against the trailing control edge.
private struct ChoiceArrow: Shape {
    func path(in rect: CGRect) -> Path {
        Path { path in
            path.move(to: CGPoint(x: rect.midX - 4, y: rect.midY - 2))
            path.addLine(to: CGPoint(x: rect.midX, y: rect.midY + 2))
            path.addLine(to: CGPoint(x: rect.midX + 4, y: rect.midY - 2))
        }
    }
}

private struct ChoiceLabelLayout: Layout {
    let ellipsis: Bool
    let presentation: EditorChoice.Presentation
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        let natural = sizes.map(\.width).max() ?? 0
        let width = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? natural
        return CGSize(width: width, height: sizes.map(\.height).max() ?? 0)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (index, view) in subviews.enumerated() {
            // Match the select's text baseline; the disclosure stays centered.
            view.place(at: CGPoint(x: bounds.minX, y: bounds.midY + (presentation == .select ? 1 : 0)), anchor: .leading,
                proposal: ProposedViewSize(width: ellipsis && index == subviews.count - 1 ? bounds.width : nil,
                    height: bounds.height))
        }
    }
}

private struct MeasureChoices: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var measureChoices: Bool {
        get { self[MeasureChoices.self] }
        set { self[MeasureChoices.self] = newValue }
    }
}
struct ChoiceFrames: PreferenceKey {
    static var defaultValue: [String: CGRect] { [:] }
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, new in new })
    }
}
private struct ChoiceMeasurement: ViewModifier {
    let id: String
    @Environment(\.measureChoices) private var measure
    func body(content: Content) -> some View {
        content.background {
            if measure {
                GeometryReader { geometry in
                    Color.clear.preference(key: ChoiceFrames.self, value: [id: geometry.frame(in: .named("choice-capture"))])
                }
            }
        }
    }
}
