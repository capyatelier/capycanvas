import SwiftUI

struct ColorSwatch: View {
    let rgba: JSON
    var body: some View {
        Canvas(colorMode: .extendedLinear) { graphics, size in
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
    /// Encoded Display P3 preview values from the shared viewing transform.
    var paintColor: Color { Color(.displayP3, red: self[0].number, green: self[1].number, blue: self[2].number,
        opacity: self[3].isNull ? 1 : self[3].number) }
}

/// Shared Rust maps artwork before native linear-light alpha composition.
struct HDRColorSwatch: View {
    let color: JSON
    let viewing: JSON
    var body: some View {
        let preview = ColorUI.resolve(["type": "hdr_preview", "color": color.raw,
            "document_space": viewing["document_space"].raw, "recipe": viewing["recipe"].raw,
            "headroom": viewing["headroom"].isNull ? 1 : viewing["headroom"].number])["linear"]
        Canvas(colorMode: .extendedLinear) { context, size in
            for row in 0..<Int(ceil(size.height / 8)) {
                for column in 0..<Int(ceil(size.width / 8)) {
                    let level = (row + column) % 2 == 0 ? 0.94 : 0.80
                    context.fill(Path(CGRect(x: column * 8, y: row * 8, width: 8, height: 8)), with: .color(Color(.sRGBLinear, white: level, opacity: 1)))
                }
            }
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(Color(.sRGBLinear,
                red: preview[0].number, green: preview[1].number, blue: preview[2].number, opacity: preview[3].number)))
        }.allowedDynamicRange(.high).clipped()
    }
}
