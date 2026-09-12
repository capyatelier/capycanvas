import CoreText
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

/// Keep fractional font advances when packing adjacent header labels. SwiftUI
/// rounds each natural Text allocation up to a pixel, accumulating visible drift
/// across a menu row. The text still uses the ordinary native system font.
@MainActor enum HeaderTextMetrics {
    enum Weight { case bold, medium }
    private struct Key: Hashable { let text: String; let size: Double; let weight: Weight }
    private static var widths: [Key: CGFloat] = [:]
    static func width(_ text: String, size: Double, weight: Weight) -> CGFloat {
        let key = Key(text: text, size: size, weight: weight)
        if let width = widths[key] { return width }
        #if canImport(AppKit)
        let font = NSFont.systemFont(ofSize: size, weight: weight == .bold ? .bold : .medium)
        #else
        let font = UIFont.systemFont(ofSize: size, weight: weight == .bold ? .bold : .medium)
        #endif
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font]))
        let width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
        // Workspace names can change throughout a long editing session.
        if widths.count >= 64 { widths.removeAll(keepingCapacity: true) }
        widths[key] = width
        return width
    }
}
