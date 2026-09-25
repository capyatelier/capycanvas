import SwiftUI

@main struct SquircleGeometryChecks {
    static func close(_ a: CGFloat, _ b: CGFloat, _ tolerance: CGFloat = 0.01) -> Bool { abs(a - b) <= tolerance }
    static func points(_ path: Path) -> [CGPoint] {
        var points: [CGPoint] = []
        path.forEach { element in
            switch element {
            case .move(let point), .line(let point): points.append(point)
            case .quadCurve, .curve: preconditionFailure("Squircles are polylines")
            case .closeSubpath: break
            }
        }
        return points
    }
    static func main() {
        let r: CGFloat = 18
        let corner = Path.squircleCorner(center: CGPoint(x: r, y: r), start: CGVector(dx: -r, dy: 0), end: CGVector(dx: 0, dy: -r))
        precondition(corner.count == 24 && close(corner.last!.x, r) && close(corner.last!.y, 0))
        let middle = corner[11]
        let inset = r * (1 - pow(2, -0.25))
        precondition(close(middle.x, inset, 0.05) && close(middle.y, inset, 0.05), "45° lies 0.159r inside each edge: \(middle)")
        let fitted = 0.54 * r * (1 - 1 / 2.0.squareRoot())
        precondition(close(middle.x, fitted, 0.05), "A 0.54r circle meets the corner at 45°")
        for point in corner {
            let x = abs(point.x - r) / r, y = abs(point.y - r) / r
            precondition(close(pow(x, 4) + pow(y, 4), 1, 0.001), "Superellipse |x|⁴ + |y|⁴ = 1: \(point)")
        }

        let tile = points(SquircleShape.tile.path(in: CGRect(x: 0, y: 0, width: 72, height: 36)))
        let bounds = tile.reduce(CGRect.null) { $0.union(CGRect(origin: $1, size: .zero)) }
        precondition(close(bounds.width, 72) && close(bounds.height, 36))
        let mirrored = tile.map { CGPoint(x: 72 - $0.x, y: $0.y) }
        precondition(tile.allSatisfy { point in mirrored.contains { close($0.x, point.x) && close($0.y, point.y) } },
            "Half-radius tiles are symmetric capsules")

        let joined = points(SquircleShape.tile(joined: "bottom").path(in: CGRect(x: 0, y: 0, width: 36, height: 36)))
        precondition(joined.contains { close($0.x, 36) && close($0.y, 36) } && joined.contains { close($0.x, 0) && close($0.y, 36) },
            "A drawer source squares its drawer-facing corners")
        precondition(!joined.contains { close($0.x, 0) && close($0.y, 0) })

        let clamped = points(SquircleShape(40).path(in: CGRect(x: 0, y: 0, width: 50, height: 30)))
        precondition(clamped.allSatisfy { $0.x >= -0.001 && $0.x <= 50.001 && $0.y >= -0.001 && $0.y <= 30.001 })
        precondition(!clamped.contains { close($0.x, 0) && close($0.y, 0) }, "Oversized radii scale down like CSS")

        let leading = SquircleShape.control.segment(0, of: 3), inner = SquircleShape.control.segment(1, of: 3)
        precondition(leading.corners == [.radius(12), .radius(0), .radius(0), .radius(12)]
            && inner.corners.allSatisfy { $0 == SquircleShape.Corner.radius(0) })
        precondition(SquircleShape.tile.segment(2, of: 3, stacked: true).corners == [.radius(0), .radius(0), .half, .half])
        precondition(SquircleShape.control.segment(0, of: 1).corners == SquircleShape.control.corners)
        let clip = SquircleShape(18).fittedClip.path(in: CGRect(x: 0, y: 0, width: 100, height: 60))
        precondition(clip.contains(CGPoint(x: 1.5, y: 1.5)) == false && clip.contains(CGPoint(x: 3.5, y: 3.5)),
            "Fitted clips are circles of 0.54r that meet the squircle at 45°")

        let toolbar = CGRect(x: 10, y: 10, width: 300, height: 36)
        let end = DrawerSource(direction: "bottom", bounds: CGRect(x: 274, y: 10, width: 36, height: 36))
        precondition(DrawerSource.square(toolbar, radius: 18, sources: [end]) == [false, false, true, false],
            "Only the facing corner the source reaches flattens")
        let middleTile = DrawerSource(direction: "bottom", bounds: CGRect(x: 140, y: 10, width: 36, height: 36))
        precondition(DrawerSource.square(toolbar, radius: 18, sources: [middleTile]) == [false, false, false, false])
        let padded = CGRect(x: 4, y: 4, width: 312, height: 48)
        precondition(DrawerSource.square(padded, radius: 18, sources: [end]) == [false, false, false, false],
            "A padded container's edge does not touch the source")
        let left = DrawerSource(direction: "left", bounds: CGRect(x: 10, y: 10, width: 36, height: 36))
        precondition(DrawerSource.square(toolbar, radius: 18, sources: [left]) == [true, false, false, true])
        precondition(DrawerSource.square(toolbar, radius: 18, sources: [], joined: JSON([false, true, false, false])) == [false, true, false, false])
        print("Squircle geometry checks passed")
    }
}
