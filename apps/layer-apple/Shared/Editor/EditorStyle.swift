import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

struct EditorPalette {
    static let sharedAccent = Color(red: 53 / 255, green: 132 / 255, blue: 228 / 255)
    let source: JSON
    subscript(_ name: String) -> Color { Color(hex: source[name].string) }
    var accent: Color { Self.sharedAccent }
    var active: Color { accent.opacity(0.22) }
}
extension Color {
    init(hex: String) {
        let value = UInt64(hex.trimmingCharacters(in: CharacterSet(charactersIn: "#")), radix: 16) ?? 0x333333
        self.init(.sRGB, red: Double((value >> 16) & 255) / 255,
            green: Double((value >> 8) & 255) / 255, blue: Double(value & 255) / 255)
    }
}
extension JSON {
    var rect: CGRect { CGRect(x: self["x"].number, y: self["y"].number, width: self["width"].number, height: self["height"].number) }
}
extension View {
    func placed(_ bounds: JSON) -> some View {
        let r = bounds.rect
        return frame(width: max(0, r.width), height: max(0, r.height), alignment: .topLeading).offset(x: r.minX, y: r.minY)
    }
}
struct SharedIcon: View {
    let name: String
    var size: CGFloat = 16
    var body: some View {
        Image("icon-" + name.replacingOccurrences(of: "layer-", with: "").replacingOccurrences(of: "-symbolic", with: ""))
            .resizable().renderingMode(.template).scaledToFit().frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}
struct IconTile: View {
    let icon: String
    let label: String
    var selected = false
    var enabled = true
    var size: CGFloat = 16
    let action: () -> Void
    var body: some View {
        Button(action: action) { SharedIcon(name: icon, size: size).frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            .buttonStyle(EditorControlButtonStyle(selected: selected))
            .disabled(!enabled).opacity(enabled ? 1 : 0.36)
            .accessibilityLabel(label).help(label)
    }
}

/// One shared disabled-opacity step is applied by the caller to the whole
/// control, including its selection. PlainButtonStyle would dim the glyph again.
struct EditorControlButtonStyle: ButtonStyle {
    var selected = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.background {
            if configuration.isPressed {
                RoundedRectangle(cornerRadius: 6).fill(.foreground).opacity(0.16)
            } else if selected {
                RoundedRectangle(cornerRadius: 6).fill(EditorPalette.sharedAccent.opacity(0.22))
            }
        }
    }
}

struct ToolActionControl: View {
    let command: JSON
    let checkable: Bool
    var textSize: CGFloat = 44 / 3
    let action: () -> Void
    private var selected: Bool { checkable && command["selected"].bool }
    private func textWidth(_ text: String) -> CGFloat {
        #if canImport(AppKit)
        let font = NSFont.systemFont(ofSize: textSize, weight: .bold)
        #else
        let font = UIFont.systemFont(ofSize: textSize, weight: .bold)
        #endif
        return (text as NSString).size(withAttributes: [.font: font]).width
    }
    var body: some View {
        Button(action: action) {
            let words = command["label"].string.split(whereSeparator: \.isWhitespace).map(String.init)
            ToolActionWords(widths: words.map(textWidth), spacing: textWidth(" ")) {
                ForEach(words.indices, id: \.self) { index in
                    Text(words[index]).font(.system(size: textSize, weight: .bold)).fixedSize()
                }
            }
                .frame(maxWidth: .infinity, minHeight: 24)
                .padding(.horizontal, 17).padding(.vertical, 5)
                .contentShape(RoundedRectangle(cornerRadius: 6))
        }.buttonStyle(EditorControlButtonStyle(selected: selected))
            .disabled(!command["enabled"].bool).opacity(command["enabled"].bool ? 1 : 0.36)
            .help(command["tooltip"].string)
            .accessibilityLabel(command["label"].string)
            .accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityValue(checkable ? (selected ? "On" : "Off") : "")
            .accessibilityIdentifier("tool-action-" + command["id"].string)
    }
}

/// The shared action labels use greedy word wrapping and 24-point line boxes.
/// SwiftUI's paragraph layout can rebalance short last lines differently from
/// the browser (for example, "Snap / to rulers" instead of "Snap to / rulers").
private struct ToolActionWords: Layout {
    // Measure before SwiftUI rounds each individual Text view to display pixels;
    // accumulated rounding can move a fitting phrase onto an extra line.
    let widths: [CGFloat]
    let spacing: CGFloat
    private func rows(_ width: CGFloat, _ subviews: Subviews) -> [(Range<Int>, CGFloat)] {
        var rows: [(Range<Int>, CGFloat)] = [], start = 0, used: CGFloat = 0
        for index in subviews.indices {
            let next = widths[index]
            if index > start && used + spacing + next > width {
                rows.append((start..<index, used)); start = index; used = 0
            }
            used += (index == start ? 0 : spacing) + next
        }
        if start < subviews.count { rows.append((start..<subviews.count, used)) }
        return rows
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let natural = widths.reduce(CGFloat(0), +)
            + CGFloat(max(0, subviews.count - 1)) * spacing
        let width = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? natural
        return CGSize(width: width, height: CGFloat(rows(width, subviews).count) * 24)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (line, row) in rows(bounds.width, subviews).enumerated() {
            var x = bounds.midX - row.1 / 2
            for index in row.0 {
                subviews[index].place(at: CGPoint(x: x, y: bounds.minY + CGFloat(line) * 24 + 12),
                    anchor: .leading, proposal: .unspecified)
                x += widths[index] + spacing
            }
        }
    }
}
