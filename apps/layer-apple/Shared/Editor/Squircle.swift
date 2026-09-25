import SwiftUI

struct SquircleShape: InsettableShape {
    enum Corner: Equatable { case radius(CGFloat), half }
    static let surfaceRadius: CGFloat = 18
    static let controlRadius: CGFloat = 12
    static let surface = SquircleShape(surfaceRadius)
    static let control = SquircleShape(controlRadius)
    static let tile = SquircleShape(corners: [.half, .half, .half, .half])

    var corners: [Corner]
    var inset: CGFloat = 0

    init(_ radius: CGFloat) { corners = Array(repeating: .radius(radius), count: 4) }
    init(corners: [Corner]) { self.corners = corners }
    init(topLeading: CGFloat = 0, topTrailing: CGFloat = 0, bottomTrailing: CGFloat = 0, bottomLeading: CGFloat = 0) {
        corners = [.radius(topLeading), .radius(topTrailing), .radius(bottomTrailing), .radius(bottomLeading)]
    }
    static func tile(joined edge: String?) -> SquircleShape { joined(edge, corner: .half) }
    static func joined(_ edge: String?, corner rounded: Corner) -> SquircleShape {
        func corner(_ edges: String...) -> Corner { edges.contains(edge ?? "") ? .radius(0) : rounded }
        return SquircleShape(corners: [corner("top", "left"), corner("top", "right"),
            corner("bottom", "right"), corner("bottom", "left")])
    }
    init(_ radius: CGFloat, square: [Bool]) {
        corners = (0..<4).map { square.indices.contains($0) && square[$0] ? .radius(0) : .radius(radius) }
    }
    static let fit: CGFloat = 0.54
    var fittedClip: FittedCorners { FittedCorners(corners: corners) }
    func segment(_ index: Int, of count: Int, stacked: Bool = false) -> SquircleShape {
        let first = index == 0, last = index == count - 1
        let kept = [first, stacked ? first : last, last, stacked ? last : first]
        return SquircleShape(corners: corners.indices.map { kept[$0] ? corners[$0] : .radius(0) })
    }

    func inset(by amount: CGFloat) -> SquircleShape {
        var shape = self; shape.inset += amount; return shape
    }

    func path(in rect: CGRect) -> Path {
        let r = rect.insetBy(dx: inset, dy: inset)
        guard r.width > 0, r.height > 0 else { return Path() }
        var radii = corners.map { corner -> CGFloat in
            switch corner {
            case .radius(let value): max(0, value - inset)
            case .half: min(r.width, r.height) / 2
            }
        }
        let sides = [(radii[0] + radii[1], r.width), (radii[1] + radii[2], r.height),
            (radii[2] + radii[3], r.width), (radii[3] + radii[0], r.height)]
        let scale = sides.reduce(CGFloat(1)) { $1.0 > $1.1 ? min($0, $1.1 / $1.0) : $0 }
        if scale < 1 { radii = radii.map { $0 * scale } }
        var p = Path()
        p.move(to: CGPoint(x: r.minX + radii[0], y: r.minY))
        p.addLine(to: CGPoint(x: r.maxX - radii[1], y: r.minY))
        p.squircle(center: CGPoint(x: r.maxX - radii[1], y: r.minY + radii[1]), start: CGVector(dx: 0, dy: -radii[1]), end: CGVector(dx: radii[1], dy: 0))
        p.addLine(to: CGPoint(x: r.maxX, y: r.maxY - radii[2]))
        p.squircle(center: CGPoint(x: r.maxX - radii[2], y: r.maxY - radii[2]), start: CGVector(dx: radii[2], dy: 0), end: CGVector(dx: 0, dy: radii[2]))
        p.addLine(to: CGPoint(x: r.minX + radii[3], y: r.maxY))
        p.squircle(center: CGPoint(x: r.minX + radii[3], y: r.maxY - radii[3]), start: CGVector(dx: 0, dy: radii[3]), end: CGVector(dx: -radii[3], dy: 0))
        p.addLine(to: CGPoint(x: r.minX, y: r.minY + radii[0]))
        p.squircle(center: CGPoint(x: r.minX + radii[0], y: r.minY + radii[0]), start: CGVector(dx: -radii[0], dy: 0), end: CGVector(dx: 0, dy: -radii[0]))
        p.closeSubpath()
        return p
    }
}

struct FittedCorners: Shape {
    let corners: [SquircleShape.Corner]
    func path(in rect: CGRect) -> Path {
        let radii = corners.map { corner -> CGFloat in
            switch corner {
            case .radius(let value): value * SquircleShape.fit
            case .half: min(rect.width, rect.height) / 2 * SquircleShape.fit
            }
        }
        if Set(radii).count == 1 { return Path(roundedRect: rect, cornerRadius: radii[0]) }
        return Path(roundedRect: rect, cornerRadii: RectangleCornerRadii(topLeading: radii[0], bottomLeading: radii[3],
            bottomTrailing: radii[2], topTrailing: radii[1]))
    }
}

extension Path {
    static let squircleSegments = 24
    static func squircleCorner(center: CGPoint, start: CGVector, end: CGVector) -> [CGPoint] {
        (1...squircleSegments).map { step in
            let angle = Double(step) * .pi / 2 / Double(squircleSegments)
            let a = CGFloat(cos(angle).squareRoot()), b = CGFloat(sin(angle).squareRoot())
            return CGPoint(x: center.x + start.dx * a + end.dx * b, y: center.y + start.dy * a + end.dy * b)
        }
    }
    mutating func squircle(center: CGPoint, start: CGVector, end: CGVector) {
        guard start != .zero || end != .zero else { return }
        for point in Path.squircleCorner(center: center, start: start, end: end) { addLine(to: point) }
    }
}

struct DrawerSource: Equatable {
    let direction: String
    let bounds: CGRect
    var anchor = JSON()
    init(direction: String, bounds: CGRect, anchor: JSON = JSON()) { self.direction = direction; self.bounds = bounds; self.anchor = anchor }
    init?(placement: JSON, anchor: JSON) {
        guard !placement.isNull, !placement["anchor"].isNull else { return nil }
        self.init(direction: placement["direction"].string, bounds: placement["anchor"].rect, anchor: anchor)
    }
    static func == (a: Self, b: Self) -> Bool {
        a.direction == b.direction && a.bounds == b.bounds && a.anchor.stableKey == b.anchor.stableKey
    }
    func opens(tile: JSON, in panel: JSON) -> Bool {
        anchor["kind"].string == "tile" && anchor["panel"].string == panel["id"].string && anchor["tile"].uint == tile["id"].uint
    }
    static func square(_ container: CGRect, radius: CGFloat, sources: [DrawerSource], joined: JSON = JSON()) -> [Bool] {
        let corners = [CGPoint(x: container.minX, y: container.minY), CGPoint(x: container.maxX, y: container.minY),
            CGPoint(x: container.maxX, y: container.maxY), CGPoint(x: container.minX, y: container.maxY)]
        return corners.indices.map { index in
            let point = corners[index]
            return joined[index].bool || sources.contains { source in
                let facing: [Int] = switch source.direction {
                case "top": [0, 1]
                case "right": [1, 2]
                case "bottom": [2, 3]
                default: [0, 3]
                }
                let vertical = source.direction == "top" || source.direction == "bottom"
                let reachX = vertical ? radius : 0.5, reachY = vertical ? 0.5 : radius, a = source.bounds
                return facing.contains(index) && point.x >= a.minX - reachX && point.x <= a.maxX + reachX
                    && point.y >= a.minY - reachY && point.y <= a.maxY + reachY
            }
        }
    }
}
