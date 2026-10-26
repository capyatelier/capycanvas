import Foundation
import CoreGraphics

/// Native adapters preserve the actual contact device. Pen is deliberately
/// distinct from mouse, including AppKit's tablet-backed mouse events.
enum ReorderDevice: String { case mouse, touch, pen }
enum ReorderSurface: Equatable {
    case tile, row, handle, headerEditor, control, swatch
    func requiresHold(_ device: ReorderDevice) -> Bool {
        switch self {
        case .tile: return true
        case .row: return device != .mouse
        case .handle, .headerEditor, .control, .swatch: return false
        }
    }
}

/// Host pickup callbacks only; model validation and transactions stay in Rust.
@MainActor struct ReorderTarget {
    let id: String
    let surface: ReorderSurface
    var canDrag = true
    var dragDevices: [ReorderDevice] = [.mouse, .touch, .pen]
    let valid: (_ dragging: Bool) -> Bool
    var openContext: (() -> Void)?
    var closeContext: (() -> Void)?
    var press: (() -> Void)?
    let begin: (CGPoint) -> Void
    let move: (CGPoint) -> Void
    let finish: (CGPoint) -> Void
    let cancel: () -> Void
}

/// One retained contact shared by the native pan and press recognizers. Native
/// recognizers supply hold timing/slop; a hold alone never creates a transaction.
@MainActor final class ReorderContact {
    private(set) var target: ReorderTarget?
    private(set) var device = ReorderDevice.mouse
    private(set) var origin = CGPoint.zero
    private(set) var held = false
    private(set) var dragging = false
    private(set) var suppressClick = false
    private var activated = false
    private(set) var generation: UInt64 = 0
    func consumeClick() -> Bool {
        let suppressed = suppressClick; suppressClick = false; return suppressed
    }
    var requiresHold: Bool { target?.surface.requiresHold(device) ?? false }

    func suppressActivation() { suppressClick = true }
    func transfer() { cancel(); suppressActivation() }
    func prepare(_ target: ReorderTarget, device: ReorderDevice, origin: CGPoint, activate: Bool = true) {
        cancel(); suppressClick = false
        generation &+= 1
        self.target = target; self.device = device; self.origin = origin
        if activate { self.activate() }
    }
    func activate() {
        guard !activated else { return }
        activated = true; target?.press?()
    }
    @discardableResult func validate() -> Bool {
        guard let target, target.valid(dragging) else { cancel(); return false }
        return true
    }
    func recognizeHold(openContext: Bool = true) {
        guard validate(), !held, !dragging else { return }
        held = true
        suppressClick = true
        if openContext && device != .mouse { target?.openContext?() }
    }
    /// Called once native movement recognition wins. A pre-hold pan cannot
    /// admit a tile or a touch/pen row, even if an adapter calls it accidentally.
    @discardableResult func move(to point: CGPoint) -> Bool {
        guard validate(), let target, target.canDrag, target.dragDevices.contains(device), !requiresHold || held else { return false }
        if !dragging {
            dragging = true; suppressClick = true
            target.closeContext?()
            target.begin(origin)
        }
        target.move(point)
        return true
    }
    func release(at point: CGPoint) {
        guard validate(), let target else { return }
        let committed = dragging
        clear()
        // A stationary recognized hold deliberately leaves its menu open.
        if committed { target.finish(point) }
    }
    func cancel() {
        let previous = target, close = held, rollback = dragging
        clear()
        if close { previous?.closeContext?() }
        if rollback { previous?.cancel() }
    }
    private func clear() { target = nil; held = false; dragging = false; activated = false }
}
