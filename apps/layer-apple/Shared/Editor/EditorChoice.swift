import SwiftUI

/// Match the web flex row: shrink content proportionally, excluding the select's
/// 20 points of CSS padding, then cap the select at 60% of the row.
struct PropertyChoiceRow: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let ideal = subviews.reduce(CGFloat(6)) { $0 + $1.sizeThatFits(.unspecified).width }
        let width = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? ideal
        return CGSize(width: width, height: 34)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 2 else { return }
        let label = subviews[0].sizeThatFits(.unspecified).width
        let ideal = subviews[1].sizeThatFits(.unspecified).width
        let content = max(0, ideal - 20)
        let shrink = min(1, max(0, bounds.width - 6 - 20) / max(1, label + content))
        let choice = min(bounds.width * 0.6, content * shrink + 20)
        subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.midY + 1), anchor: .leading,
            proposal: ProposedViewSize(width: max(0, bounds.width - choice - 6), height: bounds.height))
        subviews[1].place(at: CGPoint(x: bounds.maxX - choice, y: bounds.minY), anchor: .topLeading,
            proposal: ProposedViewSize(width: choice, height: bounds.height))
    }
}

struct EditorChoice: View {
    let label: String
    let options: [String]
    let selected: Int
    let identifier: String
    let background: Color
    var compact = false
    let select: (Int) -> Void
    @State private var choosing = false
    @Environment(\.isEnabled) private var enabled
    var body: some View {
        Button { choosing = true } label: {
            ChoiceLabelLayout(ellipsis: compact) {
                ForEach(options.indices, id: \.self) { Text(options[$0]).hidden().accessibilityHidden(true) }
                Text(options.indices.contains(selected) ? options[selected] : label).lineLimit(1)
            }.fontWeight(compact ? .bold : .regular).clipped()
                .modifier(ChoiceMeasurement(id: identifier + ":text"))
                .padding(.leading, compact ? 10 : 14).padding(.trailing, compact ? 22 : 26)
                .frame(height: compact ? 24 : 34)
                .overlay(alignment: .trailing) {
                    ChoiceArrow().stroke(style: StrokeStyle(lineWidth: 2, lineCap: .butt, lineJoin: .miter))
                        .frame(width: 16, height: 16)
                    .modifier(ChoiceMeasurement(id: identifier + ":arrow"))
                }
                .background(background, in: RoundedRectangle(cornerRadius: 6)).contentShape(RoundedRectangle(cornerRadius: 6))
        }.buttonStyle(EditorControlButtonStyle()).opacity(enabled ? 1 : 0.7)
            .modifier(ChoiceMeasurement(id: identifier))
            .accessibilityLabel(label).accessibilityValue(options.indices.contains(selected) ? options[selected] : "")
            .accessibilityIdentifier(identifier)
            .popover(isPresented: $choosing) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(options.indices, id: \.self) { index in
                            Button { choosing = false; select(index) } label: {
                                HStack(spacing: 6) {
                                    SharedIcon(name: "check").opacity(index == selected ? 1 : 0)
                                    Text(options[index])
                                    Spacer(minLength: 0)
                                }.padding(.horizontal, 8).frame(height: 28).contentShape(Rectangle())
                            }.buttonStyle(.plain).accessibilityIdentifier(identifier + "-option-\(index)")
                        }
                    }.padding(6)
                }.frame(width: 230, height: min(400, CGFloat(options.count) * 28 + 12))
                    .presentationCompactAdaptation(.popover)
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

/// Intrinsic size comes from all option labels, but the selected label alone
/// controls painting. The web's property select clips; compact selects ellipsize.
private struct ChoiceLabelLayout: Layout {
    let ellipsis: Bool
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        let natural = sizes.map(\.width).max() ?? 0
        let width = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? natural
        return CGSize(width: width, height: sizes.map(\.height).max() ?? 0)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (index, view) in subviews.enumerated() {
            // Match the select's text baseline; the disclosure stays centered.
            view.place(at: CGPoint(x: bounds.minX, y: bounds.midY + 1), anchor: .leading,
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
