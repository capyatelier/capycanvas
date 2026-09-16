import SwiftUI

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
