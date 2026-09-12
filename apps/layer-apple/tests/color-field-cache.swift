import SwiftUI

@main struct ColorFieldCacheCheck {
    static func main() {
        let cache = HLSFieldImageCache()
        precondition(cache.image(side: .nan, hue: 0) == nil)
        precondition(cache.image(side: 452, hue: .infinity) == nil)
        let first = cache.image(side: 452, hue: 150)!
        precondition(first === cache.image(side: 452, hue: 150))
        precondition(first === cache.image(side: 452.1, hue: 150))
        let data = first.dataProvider!.data! as Data
        var expected = Data(count: 452 * 452 * 4)
        precondition(expected.withUnsafeMutableBytes { buffer in
            capy_apple_hls_field(452, 150, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
        } == 1)
        precondition(data == expected)
        precondition(first !== cache.image(side: 452, hue: 151))
        precondition(first !== cache.image(side: 320, hue: 150))
        precondition(first !== cache.image(side: 452, hue: 150))
        // The provider retains the bytes after subsequent cache entries replace it.
        precondition(first.dataProvider!.data! as Data == expected)
        weak let previous = cache.image(side: 768, hue: 0)
        precondition(previous != nil)
        _ = cache.image(side: 320, hue: 0)
        precondition(previous == nil, "A replaced image must not accumulate in the cache")
        print("HLS cache: reuse, hue/size invalidation, retained bytes and single-entry lifetime passed")
    }
}
