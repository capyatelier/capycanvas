import SwiftUI

/// Shared Rust maps artwork before native linear-light alpha composition.
struct HDRColorSwatch: View {
    let color: JSON
    let viewing: JSON
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let preview = ColorUI.resolve(["type": "hdr_preview", "color": color.raw,
            "document_space": viewing["document_space"].raw, "recipe": viewing["recipe"].raw,
            "headroom": viewing["headroom"].isNull ? 1 : viewing["headroom"].number])["linear"]
        Canvas(colorMode: .extendedLinear) { [palette] context, size in
            context.fillTransparencyChecker(size, palette: palette)
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(Color(.sRGBLinear,
                red: preview[0].number, green: preview[1].number, blue: preview[2].number, opacity: preview[3].number)))
        }.allowedDynamicRange(.high).clipped()
    }
}
