import Foundation
import CoreGraphics

final class Owner {}

@main struct ColorTouchSurfaceChecks {
    @MainActor static func main() throws {
        let first = ColorTouchSurface(), surface = ColorTouchSurface()
        precondition(surface.surface != first.surface)
        var sent: [[String: Any]] = []
        var retired = 0
        let wheel = Owner(), arc = Owner(), removed = Owner()
        surface.send = { sent.append($0) }
        surface.onClaim(wheel) { retired += 1 }
        surface.onClaim(arc) { retired += 1 }
        surface.onClaim(removed) { retired += 100 }
        surface.remove(removed)
        let a = Owner(), b = Owner(), c = Owner()
        surface.contact(ObjectIdentifier(a), phase: "move", position: .zero, timestamp: 1)
        precondition(sent.isEmpty, "untracked contacts are not forwarded")
        surface.contact(ObjectIdentifier(a), phase: "down", position: CGPoint(x: 20, y: 30), timestamp: 1.5)
        precondition(!surface.claimed && retired == 0)
        surface.contact(ObjectIdentifier(b), phase: "down", position: CGPoint(x: 60, y: 30), timestamp: 1.52)
        precondition(surface.claimed && retired == 2, "the second finger retires each single-finger pick once")
        surface.contact(ObjectIdentifier(c), phase: "down", position: CGPoint(x: 40, y: 70), timestamp: 1.53)
        precondition(retired == 2)
        for key in [a, b] {
            surface.contact(ObjectIdentifier(key), phase: "up", position: CGPoint(x: 1, y: 2), timestamp: 1.6)
        }
        precondition(surface.claimed && surface.tracks(ObjectIdentifier(c)), "a staggered third finger remains claimed")
        surface.contact(ObjectIdentifier(c), phase: "up", position: CGPoint(x: 1, y: 2), timestamp: 1.7)
        precondition(!surface.claimed && !surface.tracks(ObjectIdentifier(a)))
        let decoded = try sent.map { try JSONSerialization.jsonObject(with: JSONSerialization.data(withJSONObject: $0)) as! [String: Any] }
        precondition(decoded.map { $0["phase"] as! String } == ["down", "down", "down", "up", "up", "up"])
        precondition(decoded.map { ($0["id"] as! NSNumber).uint64Value } == [1, 2, 3, 1, 2, 3])
        precondition(decoded.allSatisfy { $0["type"] as? String == "color_touch" && ($0["surface"] as! NSNumber).uint64Value == surface.surface })
        precondition((decoded[0]["position"] as! [NSNumber]).map(\.doubleValue) == [20, 30])
        precondition((decoded[0]["time_ns"] as! NSNumber).uint64Value == 1_500_000_000)
        surface.contact(ObjectIdentifier(a), phase: "down", position: .zero, timestamp: 2)
        surface.contact(ObjectIdentifier(b), phase: "down", position: .zero, timestamp: 2)
        precondition(retired == 4, "every new gesture can claim again")
        print("PASS color touch surface: shared surface ids, contact ids, claim and retirement, and the color_touch input encoding")
    }
}
