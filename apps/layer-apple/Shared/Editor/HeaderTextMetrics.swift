import CoreText
import SwiftUI
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
    private struct FontKey: Hashable { let size: Double; let weight: Weight }
    private static var widths: [Key: CGFloat] = [:]
    private static var fonts: [FontKey: CTFont] = [:]
    static func font(size: Double, weight: Weight) -> Font { Font(nativeFont(size: size, weight: weight)) }
    private static func nativeFont(size: Double, weight: Weight) -> CTFont {
        let key = FontKey(size: size, weight: weight)
        if let font = fonts[key] { return font }
        #if canImport(AppKit)
        let base = NSFont.systemFont(ofSize: size, weight: weight == .bold ? .bold : .medium)
        #else
        let base = UIFont.systemFont(ofSize: size, weight: weight == .bold ? .bold : .medium)
        #endif
        var font: CTFont = base
        // The system's named Medium instance uses weight 510. The shared web
        // label requests CSS 500; use the public variable-font axis for both
        // measurement and drawing, retaining the native font if unavailable.
        if weight == .medium, let axes = CTFontCopyVariationAxes(base) as? [[String: Any]],
           axes.contains(where: { ($0[kCTFontVariationAxisIdentifierKey as String] as? NSNumber)?.uint32Value == 0x77676874 }) {
            var variations = CTFontCopyVariation(base) as? [NSNumber: NSNumber] ?? [:]
            variations[NSNumber(value: 0x77676874)] = 500
            let descriptor = CTFontDescriptorCreateCopyWithAttributes(CTFontCopyFontDescriptor(base),
                [kCTFontVariationAttribute: variations] as CFDictionary)
            font = CTFontCreateWithFontDescriptor(descriptor, size, nil)
        }
        if fonts.count >= 16 { fonts.removeAll(keepingCapacity: true) }
        fonts[key] = font
        return font
    }
    static func width(_ text: String, size: Double, weight: Weight) -> CGFloat {
        let key = Key(text: text, size: size, weight: weight)
        if let width = widths[key] { return width }
        let font = nativeFont(size: size, weight: weight)
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font]))
        let width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
        // Workspace names can change throughout a long editing session.
        if widths.count >= 64 { widths.removeAll(keepingCapacity: true) }
        widths[key] = width
        return width
    }
}
