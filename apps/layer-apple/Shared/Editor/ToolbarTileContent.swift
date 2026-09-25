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
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.trailing, 4)
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    @ViewBuilder private var glyph: some View {
        if tile["control"]["kind"].string == "color" {
            PaintPairIcon(rgba: colors, size: iconSize)
        } else {
            SharedIcon(name: tile["icon"].string, size: iconSize)
        }
    }
}

/// The shared 16-unit Color icon with live paints: the background circle
/// under the foreground circle, each with a one-unit text-colored outline.
struct PaintPairIcon: View {
    let rgba: JSON
    let size: CGFloat
    var body: some View {
        ZStack(alignment: .topLeading) {
            swatch("background", center: 11, radius: 4.25)
            swatch("foreground", center: 6.75, radius: 6)
        }.frame(width: size, height: size, alignment: .topLeading).accessibilityHidden(true)
    }
    private func swatch(_ slot: String, center: CGFloat, radius: CGFloat) -> some View {
        let unit = size / 16
        return ColorSwatch(rgba: rgba[slot])
            .frame(width: radius * 2 * unit, height: radius * 2 * unit).clipShape(Circle())
            .overlay(Circle().stroke(.foreground, lineWidth: unit))
            .offset(x: (center - radius) * unit, y: (center - radius) * unit)
    }
}
