import SwiftUI

@main struct ColorFieldCacheCheck {
    static func main() {
        let cache = ColorFieldImageCache()
        precondition(cache.image(side: .nan, hue: 0, shape: .triangle) == nil)
        precondition(cache.image(side: 452, hue: .infinity, shape: .triangle) == nil)
        let first = cache.image(side: 452, hue: 150, shape: .triangle)!
        precondition(first === cache.image(side: 452, hue: 150, shape: .triangle))
        precondition(first === cache.image(side: 452.1, hue: 150, shape: .triangle))
        let data = first.dataProvider!.data! as Data
        var expected = Data(count: 452 * 452 * 4)
        precondition(expected.withUnsafeMutableBytes { buffer in
            capy_apple_color_field(452, 150, 2, false, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
        } == 1)
        precondition(data == expected)
        precondition(first !== cache.image(side: 452, hue: 151, shape: .triangle))
        precondition(first !== cache.image(side: 320, hue: 150, shape: .triangle))
        precondition(first !== cache.image(side: 452, hue: 150, shape: .triangle))
        // The provider retains the bytes after subsequent cache entries replace it.
        precondition(first.dataProvider!.data! as Data == expected)
        weak let previous = cache.image(side: 768, hue: 0, shape: .triangle)
        precondition(previous != nil)
        _ = cache.image(side: 320, hue: 0, shape: .triangle)
        precondition(previous == nil, "A replaced image must not accumulate in the cache")
        let circle = cache.image(side: 226, hue: 150, shape: .circle)!
        precondition(circle === cache.image(side: 226, hue: 150, shape: .circle))
        precondition(circle !== cache.image(side: 226, hue: 150, shape: .triangle))
        precondition(cache.image(side: 226, hue: 150, shape: .square) != nil)
        let ring = cache.image(side: 226, hue: 0, shape: .circle, guide: true)!
        precondition(ring === cache.image(side: 226, hue: 280, shape: .circle, guide: true), "The guide never changes during hue picking")
        precondition(ring !== cache.image(side: 226, hue: 0, shape: .circle))
        precondition(ring !== cache.image(side: 226, hue: 0, shape: .square, guide: true))
        print("Color cache: field/guide reuse, hue/size/shape invalidation, retained bytes and single-entry lifetime passed")
    }
}
