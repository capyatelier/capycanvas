import SwiftUI

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
