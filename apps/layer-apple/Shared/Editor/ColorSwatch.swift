import SwiftUI

struct ColorSwatch: View {
    let rgba: JSON
    @Environment(\.editorPalette) private var palette
    var body: some View {
        Canvas(colorMode: .extendedLinear) { [palette] graphics, size in
            graphics.fillTransparencyChecker(size, palette: palette)
            graphics.fill(Path(CGRect(origin: .zero, size: size)), with: .color(rgba.paintColor))
        }.clipped()
    }
}
extension JSON {
    /// Encoded Display P3 preview values from the shared viewing transform.
    var paintColor: Color { Color(.displayP3, red: self[0].number, green: self[1].number, blue: self[2].number,
        opacity: self[3].isNull ? 1 : self[3].number) }
}
