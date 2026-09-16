import SwiftUI

@main struct ColorFieldCacheCheck {
    static func main() {
        let cache = ColorFieldImageCache()
        precondition(cache.image(side: .nan, hue: 0, shape: .triangle, rgbSpace: "Srgb") == nil)
        precondition(cache.image(side: 452, hue: .infinity, shape: .triangle, rgbSpace: "Srgb") == nil)
        let first = cache.image(side: 452, hue: 150, shape: .triangle, rgbSpace: "Srgb")!
        precondition(first === cache.image(side: 452, hue: 150, shape: .triangle, rgbSpace: "Srgb"))
        precondition(first === cache.image(side: 452.1, hue: 150, shape: .triangle, rgbSpace: "Srgb"))
        let data = first.dataProvider!.data! as Data
        var expected = Data(count: 452 * 452 * 4)
        precondition(expected.withUnsafeMutableBytes { buffer in
            capy_apple_color_field(452, 150, 2, "Srgb", false, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
        } == 1)
        precondition(data == expected)
        precondition(first !== cache.image(side: 452, hue: 151, shape: .triangle, rgbSpace: "Srgb"))
        precondition(first !== cache.image(side: 320, hue: 150, shape: .triangle, rgbSpace: "Srgb"))
        precondition(first !== cache.image(side: 452, hue: 150, shape: .triangle, rgbSpace: "Srgb"))
        // The provider retains the bytes after subsequent cache entries replace it.
        precondition(first.dataProvider!.data! as Data == expected)
        weak let previous = cache.image(side: 768, hue: 0, shape: .triangle, rgbSpace: "Srgb")
        precondition(previous != nil)
        _ = cache.image(side: 320, hue: 0, shape: .triangle, rgbSpace: "Srgb")
        precondition(previous == nil, "A replaced image must not accumulate in the cache")
        let circle = cache.image(side: 226, hue: 150, shape: .circle, rgbSpace: "Srgb")!
        precondition(circle === cache.image(side: 226, hue: 150, shape: .circle, rgbSpace: "Srgb"))
        precondition(circle !== cache.image(side: 226, hue: 150, shape: .triangle, rgbSpace: "Srgb"))
        precondition(cache.image(side: 226, hue: 150, shape: .square, rgbSpace: "Srgb") != nil)
        let ring = cache.image(side: 226, hue: 0, shape: .circle, rgbSpace: "Srgb", guide: true)!
        precondition(ring === cache.image(side: 226, hue: 280, shape: .circle, rgbSpace: "Srgb", guide: true), "The guide never changes during hue picking")
        precondition(ring !== cache.image(side: 226, hue: 0, shape: .circle, rgbSpace: "Srgb"))
        precondition(ring !== cache.image(side: 226, hue: 0, shape: .square, rgbSpace: "Srgb", guide: true))
        for guide in [false, true] {
            let srgb = cache.image(side: 226, hue: 42, shape: .square, rgbSpace: "Srgb", guide: guide)!
            let p3 = cache.image(side: 226, hue: 42, shape: .square, rgbSpace: "DisplayP3", guide: guide)!
            precondition(srgb !== p3, "Document gamut changes must invalidate the field and guide")
            precondition(srgb.dataProvider!.data! as Data != p3.dataProvider!.data! as Data,
                "Wide-gamut hue/field previews must be converted before display")
            precondition(p3 === cache.image(side: 226, hue: 42, shape: .square, rgbSpace: "DisplayP3", guide: guide))
            for space in ["AdobeRgb", "ProPhoto"] {
                precondition(cache.image(side: 226, hue: 42, shape: .square, rgbSpace: space, guide: guide) != nil)
            }
            precondition(cache.image(side: 226, hue: 42, shape: .square, rgbSpace: "Unknown", guide: guide) == nil)
        }
        print("Color cache: field/guide reuse, hue/size/shape/space invalidation, converted gamut, retained bytes and single-entry lifetime passed")
    }
}
