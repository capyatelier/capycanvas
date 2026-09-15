import SwiftUI

struct BrushColorButton: View {
    @ObservedObject var store: EditorStore
    let label: String
    var body: some View {
        Button { store.customize(["type": "open_control", "control": "brush_color"]) } label: {
            ColorSwatch(rgba: store.state["brush"]["color"])
                .clipShape(RoundedRectangle(cornerRadius: 4))
                .padding(.horizontal, 12).padding(.vertical, 4).frame(height: 34)
                .background(EditorPalette(source: store.state["palette"])["button"].opacity(13 / 255),
                    in: RoundedRectangle(cornerRadius: 6))
                .contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(label).accessibilityIdentifier("brush-color")
    }
}

struct ColorSwatch: View {
    let rgba: JSON
    var body: some View {
        Canvas { graphics, size in
            for row in 0..<Int(ceil(size.height / 5)) {
                for column in 0..<Int(ceil(size.width / 5)) {
                    let level = (row + column) % 2 == 0 ? 0.8 : 0.55
                    graphics.fill(Path(CGRect(x: column * 5, y: row * 5, width: 5, height: 5)), with: .color(Color(.sRGB, white: level, opacity: 1)))
                }
            }
            graphics.fill(Path(CGRect(origin: .zero, size: size)), with: .color(rgba.paintColor))
        }.clipped()
    }
}
extension JSON {
    var paintColor: Color { Color(.sRGB, red: self[0].number, green: self[1].number, blue: self[2].number,
        opacity: self[3].isNull ? 1 : self[3].number) }
}
