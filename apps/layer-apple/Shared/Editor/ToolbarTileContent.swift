import SwiftUI

struct ToolbarTileButton: View {
    let panel: JSON
    let tile: JSON
    let palette: EditorPalette
    let colors: JSON
    var drawerOpen = false
    var drawerDirection: String?
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            ToolbarTileContent(panel: panel, tile: tile, palette: palette, colors: colors)
                .overlay(alignment: .bottomTrailing) { if tile["has_variants"].bool { ToolGroupMarker() } }
                .contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle(selected: tile["selected"].bool, joinedEdge: drawerOpen ? drawerDirection : nil,
            drawerBackground: drawerOpen ? .clear : nil, corner: .half))
            .foregroundStyle(palette["text"])
            .disabled(!tile["enabled"].bool).opacity(tile["enabled"].bool ? 1 : 0.36)
    }
}

/// Rust supplies style metrics for ribbons, floating panels, drawers and Zen.
/// Labels use the same 36-point icon column and trailing 72-point text column
/// as the browser and Android; tile bounds and hit targets stay in shared layout.
struct ToolbarTileContent: View {
    let panel: JSON
    let tile: JSON
    let palette: EditorPalette
    let colors: JSON
    private var iconSize: CGFloat { CGFloat(panel["tile_icon_size"].number) }
    private var labelLines: Int { Int(panel["tile_label_lines"].number) }
    var body: some View {
        HStack(spacing: 0) {
            glyph.frame(width: labelLines > 0 ? 36 : iconSize)
            if labelLines > 0 {
                Text(tile["label"].string)
                    .fontWeight(panel["tile_label_bold"].bool ? .bold : .regular)
                    .lineLimit(labelLines).truncationMode(.tail).multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.trailing, tile["has_variants"].bool ? 16 : 4)
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    @ViewBuilder private var glyph: some View {
        if (tile["resolved_control"].isNull ? tile["control"] : tile["resolved_control"])["kind"].string == "color" {
            PaintPairIcon(pair: colors, size: iconSize)
        } else {
            SharedIcon(name: tile["icon"].string, size: iconSize)
        }
    }
}

struct ToolGroupMarker: View {
    var body: some View {
        SharedIcon(name: "tool-group").allowsHitTesting(false).accessibilityHidden(true)
    }
}

struct PaintPairIcon: View {
    let pair: JSON
    let size: CGFloat
    var body: some View {
        let front = pair["front_swatch"].string
        Canvas { graphics, _ in
            graphics.scaleBy(x: size / 16, y: size / 16)
            for swatch in pair["swatches"].array.sorted(by: { ($0["slot"].string == front ? 1 : 0) < ($1["slot"].string == front ? 1 : 0) }) {
                let foreground = swatch["slot"].string == "foreground"
                let center: CGFloat = foreground ? 6.75 : 11, radius: CGFloat = foreground ? 6 : 4.25
                let field = CGRect(x: center - radius, y: center - radius, width: radius * 2, height: radius * 2)
                let cell = CGFloat(pair["checker_cell"].number), count = Int((radius * 2 / max(cell, 0.5)).rounded(.up))
                var layer = graphics
                layer.clip(to: Path(ellipseIn: field))
                layer.fill(Path(field), with: .color(swatch["checker"][0].paintColor))
                var odd = Path()
                for y in 0..<count { for x in 0..<count where (x + y) % 2 == 1 {
                    odd.addRect(CGRect(x: field.minX + CGFloat(x) * cell, y: field.minY + CGFloat(y) * cell, width: cell, height: cell))
                } }
                layer.fill(odd, with: .color(swatch["checker"][1].paintColor))
                graphics.stroke(Path(ellipseIn: field), with: .foreground, lineWidth: 1)
            }
        }.frame(width: size, height: size).accessibilityHidden(true)
    }
}
