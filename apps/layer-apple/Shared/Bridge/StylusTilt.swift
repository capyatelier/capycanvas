import CoreGraphics

enum StylusTilt {
    static func towardBarrel(altitude: CGFloat, azimuth: CGFloat) -> (x: Double, y: Double) {
        (Double(atan2(cos(altitude) * cos(azimuth), sin(altitude))), Double(atan2(cos(altitude) * sin(azimuth), sin(altitude))))
    }
}
