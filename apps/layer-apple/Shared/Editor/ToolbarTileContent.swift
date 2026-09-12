import SwiftUI

struct ToolbarTileButton: View {
    let panel: JSON
    let tile: JSON
    let palette: EditorPalette
    let color: JSON
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            ToolbarTileContent(panel: panel, tile: tile, palette: palette, color: color)
                .contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle(selected: tile["selected"].bool))
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
    let color: JSON
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
            // Dynamic fill within the canonical color icon's 16-unit viewbox.
            let scale = iconSize / 16
            ColorSwatch(rgba: color)
                .frame(width: 11 * scale, height: 11 * scale).clipShape(Circle())
                .overlay(Circle().stroke(palette["text"], lineWidth: 1.5 * scale))
                .frame(width: iconSize, height: iconSize)
        } else {
            SharedIcon(name: tile["icon"].string, size: iconSize)
        }
    }
}
