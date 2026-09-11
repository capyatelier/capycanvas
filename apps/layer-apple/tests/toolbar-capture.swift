// Render actual shared SwiftUI tile buttons without launching the drawing app.
// Run from a temporary .app bundle containing the built app's Assets.car.
import AppKit
import SwiftUI

@main struct ToolbarCapture {
    @MainActor static func main() throws {
        precondition(CommandLine.arguments.count == 3, "Expected fixture.json and output.png")
        let fixture = try JSON.decode(String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8))
        precondition(fixture["schema"].uint == 1)
        let palette = EditorPalette(source: fixture["palette"])
        let width = fixture["width"].number, height = fixture["height"].number
        // Fail clearly if the shared vector assets are unavailable to Bundle.main.
        precondition(NSImage(named: "icon-pen") != nil, "Missing compiled shared Assets.car")
        let content = VStack(spacing: 6) {
            ForEach(fixture["rows"].array.indices, id: \.self) { index in
                let row = fixture["rows"][index], panel = row["panel"]
                HStack(spacing: 2) {
                    ForEach(panel["tiles"].array.indices, id: \.self) { tile in
                        ToolbarTileButton(panel: panel, tile: panel["tiles"][tile], palette: palette, color: fixture["color"]) {}
                            .frame(width: row["size"][0].number, height: row["size"][1].number)
                    }
                }.frame(width: width - 12, alignment: .leading)
            }
        }.padding(6).frame(width: width, height: height, alignment: .topLeading)
            .font(.system(size: fixture["text_size"].number)).foregroundStyle(palette["text"])
            .background(palette["panel"])
        let renderer = ImageRenderer(content: content)
        renderer.scale = 2; renderer.proposedSize = ProposedViewSize(width: width, height: height)
        guard let image = renderer.cgImage else { fatalError("No rendered toolbar image") }
        let bitmap = NSBitmapImageRep(cgImage: image)
        try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
        print("Rendered five shared tile styles at 2× scale")
    }
}
