// Capture real editor icon/tile buttons under contrasting system accents.
// State combinations include selected+disabled, which ordinary startup omits.
import AppKit
import SwiftUI

@main struct ControlColorCaptures {
    @MainActor static func main() throws {
        precondition(CommandLine.arguments.count == 3, "Expected fixture.json and output.png")
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let fixture = try JSON.decode(String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8))
        let palette = EditorPalette(source: fixture["palette"])
        precondition(NSImage(named: "icon-pen") != nil, "Missing compiled shared assets")
        let states = [(false, true), (true, true), (false, false), (true, false)]
        let accents: [(String, Color?)] = [("system", nil), ("red", .red), ("green", .green)]
        let panel = fixture["rows"][0]["panel"], original = panel["tiles"][0]
        let width = 168.0, height = 258.0
        let content = VStack(spacing: 6) {
            ForEach(0..<6) { row in
                HStack(spacing: 4) {
                    ForEach(states.indices, id: \.self) { column in
                        let (selected, enabled) = states[column]
                        Group {
                            if row < 3 {
                                IconTile(icon: original["icon"].string, label: original["label"].string,
                                    selected: selected, enabled: enabled) {}
                            } else {
                                let tile = original.replacing("selected", with: JSON(selected)).replacing("enabled", with: JSON(enabled))
                                ToolbarTileButton(panel: panel, tile: tile, palette: palette, color: fixture["color"]) {}
                            }
                        }.frame(width: 36, height: 36)
                    }
                }.tint(palette.accent).accentColor(accents[row % 3].1)
            }
        }.padding(6).frame(width: width, height: height, alignment: .topLeading)
            .environment(\.colorScheme, fixture["theme"].string == "dark" ? .dark : .light)
            .font(.system(size: fixture["text_size"].number)).foregroundStyle(palette["text"])
            .background(palette["panel"])
        let renderer = ImageRenderer(content: content)
        renderer.scale = 2; renderer.proposedSize = ProposedViewSize(width: width, height: height)
        guard let image = renderer.cgImage else { throw HostFailure(message: "No control image") }
        let bitmap = NSBitmapImageRep(cgImage: image)
        let output = URL(fileURLWithPath: CommandLine.arguments[2])
        try bitmap.representation(using: .png, properties: [:])!.write(to: output)
        let rows = (0..<6).map { row in
            ["kind": row < 3 ? "icon" : "toolbar", "accent": accents[row % 3].0,
             "cells": states.enumerated().map { index, state in
                ["selected": state.0, "enabled": state.1,
                 "bounds": ["x": 6 + index * 40, "y": 6 + row * 42, "width": 36, "height": 36]] as [String: Any]
             }] as [String: Any]
        }
        try JSON(["schema": 1, "width": width, "height": height, "scale": 2,
            "theme": fixture["theme"].raw, "palette": fixture["palette"].raw,
            "color": fixture["color"].raw, "text_size": fixture["text_size"].raw,
            "panel": panel.raw, "tile": original.raw, "rows": rows]).encoded()
            .write(to: output.deletingPathExtension().appendingPathExtension("json"), atomically: true, encoding: .utf8)
        print("Captured 24 real icon/toolbar controls with enabled, selected and accent variations")
    }
}
