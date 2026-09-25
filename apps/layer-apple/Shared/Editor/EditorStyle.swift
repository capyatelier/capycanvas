import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

struct EditorPalette {
    let source: JSON
    subscript(_ name: String) -> Color { Color(hex: source[name].string) }
    private func role(_ name: String, _ fallback: String) -> Color { Color(hex: source[name].isNull ? fallback : source[name].string) }
    var accent: Color { role("accent", "#3584e4") }
    var accentForeground: Color { role("accent_foreground", "#ffffff") }
    var active: Color { role("selection", "#c0d7f6") }
    var headerSelection: Color { role("header_selection", "#afc6e5") }
    var headerSelectionHover: Color { role("header_selection_hover", "#a6bddb") }
    var chromeSurface: Color { self["bg"].opacity(0.75) }
    var checkerLight: Color { role("checker_light", "#dcdcdc") }
    var checkerDark: Color { role("checker_dark", "#aaaaaa") }
}
extension GraphicsContext {
    func fillTransparencyChecker(_ size: CGSize, palette: EditorPalette) {
        let cell: CGFloat = 5
        for row in 0..<Int(ceil(size.height / cell)) {
            for column in 0..<Int(ceil(size.width / cell)) {
                fill(Path(CGRect(x: CGFloat(column) * cell, y: CGFloat(row) * cell, width: cell, height: cell)),
                    with: .color((row + column) % 2 == 0 ? palette.checkerLight : palette.checkerDark))
            }
        }
    }
}
private struct EditorPaletteKey: EnvironmentKey { static let defaultValue = EditorPalette(source: JSON()) }
extension EnvironmentValues {
    var editorPalette: EditorPalette {
        get { self[EditorPaletteKey.self] }
        set { self[EditorPaletteKey.self] = newValue }
    }
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
/// Shared grip orientation and inset for panel headers and column footers.
struct PanelGrip: View {
    var vertical = false
    var body: some View {
        SharedIcon(name: "grip").opacity(0.65)
            .rotationEffect(.degrees(vertical ? 90 : 0))
            .offset(x: vertical ? 0 : -1.6, y: vertical ? -1.6 : 0)
    }
}

struct SharedIcon: View {
    let name: String
    var size: CGFloat = 16
    var body: some View {
        let key = Self.assetKey(name)
        Group {
            if let layers = Self.layers[key] {
                ZStack {
                    ForEach(layers, id: \.asset) { layer in
                        glyph(layer.asset, template: layer.template)
                    }
                }.compositingGroup()
            } else {
                glyph("icon-" + key, template: true)
            }
        }.accessibilityHidden(true)
    }
    static func assetKey(_ name: String) -> String {
        var key = name
        if key.hasPrefix("layer-") { key.removeFirst("layer-".count) }
        if key.hasSuffix("-symbolic") { key.removeLast("-symbolic".count) }
        return key
    }
    private func glyph(_ asset: String, template: Bool) -> some View {
        Image(asset).resizable().renderingMode(template ? .template : .original)
            .scaledToFit().frame(width: size, height: size)
    }
    private struct Layer: Decodable { let asset: String; let template: Bool }
    // The generated data asset shares Assets.car with the vectors on both hosts.
    // Load once; fixed paints and foreground masks retain SVG painter order.
    private static let layers: [String: [Layer]] = {
        guard let data = NSDataAsset(name: "shared-icon-paints")?.data else { return [:] }
        return (try? JSONDecoder().decode([String: [Layer]].self, from: data)) ?? [:]
    }()
}
struct IconTile: View {
    let icon: String
    let label: String
    var selected = false
    var enabled = true
    var size: CGFloat = 16
    var active = false
    var joinedEdge: String?
    var background: Color?
    var keepsBackground = false
    var drawerBackground: Color?
    let action: () -> Void
    var body: some View {
        Button(action: action) { SharedIcon(name: icon, size: size).frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            .buttonStyle(EditorControlButtonStyle(selected: selected, active: active, joinedEdge: joinedEdge,
                background: background, keepsBackground: keepsBackground, drawerBackground: drawerBackground))
            .disabled(!enabled).opacity(enabled ? 1 : 0.36)
            .accessibilityLabel(label).help(label)
            .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// One shared disabled-opacity step is applied by the caller to the whole
/// control, including its selection. PlainButtonStyle would dim the glyph again.
struct EditorControlButtonStyle: ButtonStyle {
    var selected = false
    var active = false
    var joinedEdge: String?
    var background: Color?
    var keepsBackground = false
    var drawerBackground: Color?
    private var shape: UnevenRoundedRectangle {
        UnevenRoundedRectangle(topLeadingRadius: joinedEdge == "top" || joinedEdge == "left" ? 0 : 6,
            bottomLeadingRadius: joinedEdge == "bottom" || joinedEdge == "left" ? 0 : 6,
            bottomTrailingRadius: joinedEdge == "bottom" || joinedEdge == "right" ? 0 : 6,
            topTrailingRadius: joinedEdge == "top" || joinedEdge == "right" ? 0 : 6)
    }
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, style: self) }
    private struct Face: View {
        let configuration: Configuration
        let style: EditorControlButtonStyle
        @Environment(\.editorPalette) private var palette
        var body: some View {
            configuration.label.background {
                ZStack {
                    if let background = style.background, style.keepsBackground || !(style.selected || configuration.isPressed || style.active) {
                        style.shape.fill(background)
                    }
                    if style.selected {
                        style.shape.fill(palette.active)
                    } else if let drawerBackground = style.drawerBackground {
                        style.shape.fill(drawerBackground)
                    } else if configuration.isPressed || style.active {
                        style.shape.fill(.foreground).opacity(configuration.isPressed ? 0.16 : 0.10)
                    }
                }
            }
        }
    }
}

/// Title-bar controls: one translucent surface, or none inside a joined bar,
/// with full-height feedback in the bar and the drawer origin filling the tile.
struct HeaderButtonStyle: ButtonStyle {
    var selected = false
    var hovering = false
    var inBar = false
    var drawerOpen = false
    let radius: CGFloat
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, style: self) }
    private struct Face: View {
        let configuration: Configuration
        let style: HeaderButtonStyle
        @Environment(\.editorPalette) private var palette
        var body: some View {
            let r = style.radius
            let shape = style.drawerOpen ? SquircleShape(topLeading: r, topTrailing: r) : SquircleShape(r)
            configuration.label.background {
                ZStack {
                    if !style.inBar { shape.fill(palette.chromeSurface) }
                    Group {
                        if style.selected {
                            shape.fill(palette.headerSelection)
                        } else if style.drawerOpen {
                            shape.fill(palette["panel"])
                        } else if configuration.isPressed || style.hovering {
                            shape.fill(.foreground).opacity(configuration.isPressed ? 0.16 : 0.10)
                        }
                    }.padding(.vertical, style.inBar && !style.drawerOpen ? 1 : 0)
                }
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
            HStack(spacing: 6) {
                SharedIcon(name: command["icon"].string)
                ToolActionWords(widths: words.map(textWidth), spacing: textWidth(" ")) {
                    ForEach(words.indices, id: \.self) { index in
                        Text(words[index]).font(.system(size: textSize, weight: .bold)).fixedSize()
                    }
                }
            }
                .frame(minWidth: 0, maxWidth: .infinity, minHeight: 24, alignment: .leading)
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
        // A browser flex item's text span shrinks to its longest word, and
        // retains its natural width when there is room beside the icon.
        let available = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? natural
        let width = max(widths.max() ?? 0, min(natural, available))
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

/// Local drafts keep text/caret stable while the serial owner publishes edits.
struct EditorTextField: View {
    let label: String
    let value: String
    let edit: (String) -> Void
    @State private var text: String
    @State private var pending: String?
    init(_ label: String, value: String, edit: @escaping (String) -> Void) {
        self.label = label; self.value = value; self.edit = edit
        _text = State(initialValue: value)
    }
    var body: some View {
        TextField(label, text: Binding(get: { text }, set: { next in
            // Native fields can resend their value when editing ends.
            guard next != text else { return }
            text = next; pending = next; edit(next)
        }))
            .onChange(of: value) { _, next in
                if pending == nil || pending == next { text = next; pending = nil }
            }
    }
}

/// Submit the focused field before a containing dialog closes.
private struct EditorTextCommit: FocusedValueKey { typealias Value = () -> Void }
extension FocusedValues {
    var editorTextCommit: (() -> Void)? {
        get { self[EditorTextCommit.self] }
        set { self[EditorTextCommit.self] = newValue }
    }
}
