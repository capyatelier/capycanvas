// Exercise UIKit adapter callbacks directly; UIKit owns gesture recognition.
// This checks session ownership, cancellation and our shared row actions without
// synthesizing the physical finger/Pencil menu-to-drag handoff.
import UIKit

@MainActor private final class Contact: UITouch {
    var sourceView: UIView?
    var device: UITouch.TouchType = .direct
    override var view: UIView? { sourceView }
    override var type: UITouch.TouchType { device }
    override var timestamp: TimeInterval { 1 }
    override func location(in view: UIView?) -> CGPoint { CGPoint(x: 100, y: 20) }
}

@MainActor private class Session: NSObject, UIDragDropSession {
    var items: [UIDragItem] = []
    var point = CGPoint(x: 100, y: 20)
    weak var coordinateView: UIView?
    var allowsMoveOperation = true
    var isRestrictedToDraggingApplication = true
    func location(in view: UIView) -> CGPoint { coordinateView.map { view.convert(point, from: $0) } ?? point }
    func hasItemsConforming(toTypeIdentifiers typeIdentifiers: [String]) -> Bool { true }
    func canLoadObjects(ofClass aClass: any NSItemProviderReading.Type) -> Bool { true }
}
@MainActor private final class DragSession: Session, UIDragSession { var localContext: Any? }
@MainActor private final class DropSession: Session, UIDropSession {
    var localDragSession: (any UIDragSession)?
    nonisolated let progress = Progress(totalUnitCount: 0)
    var progressIndicatorStyle = UIDropSessionProgressIndicatorStyle.none
    func loadObjects(ofClass aClass: any NSItemProviderReading.Type,
        completion: @escaping ([any NSItemProviderReading]) -> Void) -> Progress { completion([]); return progress }
    init(_ drag: DragSession) { super.init(); localDragSession = drag; items = drag.items; point = CGPoint(x: 100, y: 119) }
}
@MainActor private final class MenuAnimator: NSObject, UIContextMenuInteractionAnimating {
    var previewViewController: UIViewController?
    var completions: [() -> Void] = []
    func addAnimations(_ animations: @escaping () -> Void) { animations() }
    func addCompletion(_ completion: @escaping () -> Void) { completions.append(completion) }
    func finish() { let pending = completions; completions = []; pending.forEach { $0() } }
}
@MainActor private final class LiftAnimator: NSObject, UIDragAnimating {
    var completions: [(UIViewAnimatingPosition) -> Void] = []
    func addAnimations(_ animations: @escaping () -> Void) { animations() }
    func addCompletion(_ completion: @escaping (UIViewAnimatingPosition) -> Void) { completions.append(completion) }
    func finish(_ position: UIViewAnimatingPosition) { completions.forEach { $0(position) }; completions = [] }
}
@MainActor private final class Fixture {
    let model = WorkspaceRowInteraction()
    let scroll = UIScrollView(frame: CGRect(x: 0, y: 0, width: 300, height: 200))
    let input = ReorderInputView(frame: CGRect(x: 0, y: 0, width: 300, height: 200))
    lazy var drag = UIDragInteraction(delegate: adapter)
    lazy var drop = UIDropInteraction(delegate: adapter)
    lazy var context = UIContextMenuInteraction(delegate: adapter)
    lazy var adapter: NativeRowMenuCoordinator = {
        input.validate()
        return scroll.interactions.compactMap { ($0 as? UIDragInteraction)?.delegate as? NativeRowMenuCoordinator }.first!
    }()
    var commits = 0
    init() {
        input.model = model
        scroll.contentSize = input.bounds.size; scroll.addSubview(input)
        model.viewport = CGRect(x: 0, y: 0, width: 300, height: 200)
        model.items = ["A", "B", "C"].map { JSON(["id": $0, "switcher_actions": [["label": "Move down", "enabled": true, "action": ["type": "move", "id": $0]]]]) }
        for (i, id) in ["A", "B", "C"].enumerated() {
            model.frames[id] = WorkspaceRowFrame(row: CGRect(x: 0, y: i * 40, width: 300, height: 40),
                grip: CGRect(x: 0, y: i * 40, width: 20, height: 40), options: CGRect(x: 280, y: i * 40, width: 20, height: 40))
        }
        model.commit = { [weak self] id, before in
            precondition(id == "A" && before == nil); self?.commits += 1
        }
    }
    func longList() {
        let ids = ["A"] + (1..<24).map { "Row \($0)" }
        model.items = ids.map { JSON(["id": $0, "switcher_actions": []]) }
        model.frames = [:]
        for (i, id) in ids.enumerated() {
            model.frames[id] = WorkspaceRowFrame(row: CGRect(x: 0, y: i * 56, width: 300, height: 56),
                grip: CGRect(x: 0, y: i * 56, width: 20, height: 56), options: CGRect(x: 280, y: i * 56, width: 20, height: 56))
        }
        input.frame.size.height = CGFloat(ids.count * 56); scroll.contentSize = input.bounds.size
    }
    func prepare() {
        let point = CGPoint(x: 100, y: 20)
        model.contact.prepare(model.source(at: point)!, device: .pen, origin: point)
    }
    func session() -> DragSession {
        precondition(adapter.responds(to: NSSelectorFromString("dragInteraction:session:didEndWithOperation:")), "UIKit must be able to call our end delegate")
        prepare(); let session = DragSession()
        session.items = adapter.dragInteraction(drag, itemsForBeginning: session)
        precondition(session.items.count == 1)
        return session
    }
    func menu() -> UIContextMenuConfiguration {
        let config = adapter.contextMenuInteraction(context, configurationForMenuAtLocation: CGPoint(x: 100, y: 20))!
        adapter.contextMenuInteraction(context, willDisplayMenuFor: config, animator: nil)
        return config
    }
}

@MainActor private func checks() async {
    for device: UITouch.TouchType in [.direct, .pencil, .indirectPointer] {
        let f = Fixture(), contact = Contact(), recognizer = UIPanGestureRecognizer()
        f.input.validate(); contact.device = device
        contact.sourceView = UIView() // Another panel above the measured rows.
        _ = f.input.gestureRecognizer(recognizer, shouldReceive: contact)
        precondition(f.model.contact.target == nil, "A covered row cannot claim another view's contact")
        let row = UIView(frame: f.scroll.bounds); f.scroll.addSubview(row)
        contact.sourceView = row
        _ = f.input.gestureRecognizer(recognizer, shouldReceive: contact)
        precondition(f.model.contact.target?.id == "A", "A visible row must retain native pickup")
        precondition(f.model.contact.device == (device == .pencil ? .pen : device == .direct ? .touch : .mouse))
        f.adapter.cancel()
    }
    do {
        let f = Fixture(), session = f.session(), config = f.menu(), animator = MenuAnimator()
        f.adapter.contextMenuInteraction(f.context, willEndFor: config, animator: animator)
        f.adapter.dragInteraction(f.drag, sessionWillBegin: session)
        animator.finish()
        precondition(f.model.contact.dragging && f.model.nativeDragging, "Menu dismissal cannot cancel its continuing drag")
        let drop = DropSession(session)
        precondition(f.adapter.dropInteraction(f.drop, canHandle: drop))
        f.adapter.dropInteraction(f.drop, performDrop: drop)
        f.adapter.dropInteraction(f.drop, performDrop: drop)
        f.adapter.dragInteraction(f.drag, session: session, didEndWith: .move)
        precondition(f.commits == 1 && !f.model.nativeDragging && f.model.contact.target == nil, "One native drop commits once")
    }
    do {
        let f = Fixture(), old = f.session()
        f.adapter.dragInteraction(f.drag, sessionWillBegin: old)
        f.adapter.cancel()
        precondition(f.commits == 0 && f.model.contact.target == nil && !f.model.nativeDragging)
        let next = f.session(); f.adapter.dragInteraction(f.drag, sessionWillBegin: next)
        f.adapter.dragInteraction(f.drag, session: old, didEndWith: .cancel)
        f.adapter.dropInteraction(f.drop, performDrop: DropSession(old))
        precondition(f.model.contact.dragging && f.model.nativeDragging && f.commits == 0, "Late callbacks from a cancelled session cannot retire a newer drag")
        f.adapter.dropInteraction(f.drop, performDrop: DropSession(next))
        precondition(f.commits == 1)
    }
    do {
        let f = Fixture(); f.prepare(); _ = f.menu()
        f.adapter.cancel()
        precondition(f.model.contact.target == nil && !f.adapter.isMenuVisible, "Cancelling a menu without a drag must release its held contact")
    }
    do {
        let f = Fixture(); f.prepare(); let menu = f.menu()
        f.adapter.contextMenuInteraction(f.context, willEndFor: menu, animator: nil)
        precondition(f.model.contact.target == nil, "No-animation dismissal still releases the held contact")
    }
    do {
        let f = Fixture(), session = f.session()
        f.adapter.dragInteraction(f.drag, sessionWillBegin: session)
        f.model.update(items: [], enabled: true); f.adapter.validate()
        f.adapter.dropInteraction(f.drop, performDrop: DropSession(session))
        precondition(f.commits == 0 && !f.model.nativeDragging && f.model.contact.target == nil, "Removing a source cancels feedback and rejects a late drop")
    }
    do {
        let f = Fixture(), session = f.session(), lift = LiftAnimator()
        precondition(f.adapter.dropInteraction(f.drop, canHandle: DropSession(session)), "A pending local lift can advertise a compatible destination")
        f.adapter.dropInteraction(f.drop, performDrop: DropSession(session))
        precondition(f.commits == 0 && !f.model.contact.dragging, "A lift alone cannot start or commit a reorder")
        f.adapter.dragInteraction(f.drag, willAnimateLiftWith: lift, session: session)
        lift.finish(.start)
        precondition(f.model.contact.target == nil && f.commits == 0, "A cancelled lift without a menu releases the contact")
    }
    do {
        let f = Fixture(), session = f.session(), menu = f.menu(), lift = LiftAnimator()
        f.adapter.dragInteraction(f.drag, willAnimateLiftWith: lift, session: session)
        lift.finish(.start)
        precondition(f.adapter.isMenuVisible && f.model.contact.held && !f.model.nativeDragging,
            "Stationary hold/release retains its menu without starting a reorder")
        f.adapter.contextMenuInteraction(f.context, willEndFor: menu, animator: nil)
        precondition(f.model.contact.target == nil)
    }
    do {
        let f = Fixture(), session = f.session(), menu = f.menu()
        f.adapter.contextMenuInteraction(f.context, willEndFor: menu, animator: nil)
        f.adapter.dragInteraction(f.drag, sessionWillBegin: session)
        precondition(f.model.contact.dragging, "A no-animation menu dismissal must preserve a pending native lift")
        f.adapter.detach()
        f.adapter.dropInteraction(f.drop, performDrop: DropSession(session))
        precondition(f.model.nativeDragging && f.commits == 0,
            "Detachment must reject late drops without publishing during SwiftUI teardown")
        await withCheckedContinuation { continuation in DispatchQueue.main.async { continuation.resume() } }
        precondition(f.model.contact.target == nil && !f.model.nativeDragging && f.commits == 0)
    }
    do {
        let f = Fixture(), session = f.session()
        f.adapter.dragInteraction(f.drag, sessionWillBegin: session)
        f.adapter.detach()
        f.prepare()
        let generation = f.model.contact.generation
        await withCheckedContinuation { continuation in DispatchQueue.main.async { continuation.resume() } }
        precondition(f.model.contact.target?.id == "A" && f.model.contact.generation == generation,
            "Deferred teardown must preserve a replacement contact")
    }
    do {
        let f = Fixture(), old = f.session(), lift = LiftAnimator()
        f.adapter.dragInteraction(f.drag, willAnimateLiftWith: lift, session: old)
        f.adapter.cancel()
        let next = f.session(); f.adapter.dragInteraction(f.drag, sessionWillBegin: next)
        lift.finish(.start)
        precondition(f.model.nativeDragging && f.model.contact.dragging, "Late lift animation must not cancel a newer session")
        f.adapter.dragInteraction(f.drag, session: next, didEndWith: .cancel)
        precondition(f.commits == 0 && !f.model.nativeDragging && f.model.contact.target == nil)
    }
    do {
        let f = Fixture(), session = f.session()
        f.adapter.dragInteraction(f.drag, sessionWillBegin: session)
        let point = CGPoint(x: 100, y: 20)
        f.model.contact.prepare(f.model.source(at: point)!, device: .mouse, origin: point)
        precondition(!f.model.nativeDragging, "Replacing a native drag restores the custom preview path")
        f.adapter.validate()
        precondition(f.model.contact.target != nil, "Native invalidation cannot cancel its replacement contact")
    }
    do {
        let f = Fixture(); f.longList()
        let container = UIView(frame: f.scroll.frame); container.addSubview(f.scroll)
        let session = f.session(); session.coordinateView = container
        f.adapter.dragInteraction(f.drag, sessionWillBegin: session)
        session.point.y = 190
        f.adapter.dragInteraction(f.drag, sessionDidMove: session)
        try? await Task.sleep(for: .milliseconds(500))
        precondition(f.scroll.contentOffset.y > 100 && f.model.contact.target?.id == "A",
            "A stationary native drag at the lower edge must scroll while retaining its offscreen source")
        let lowerOffset = f.scroll.contentOffset.y
        session.point.y = 250
        f.adapter.dragInteraction(f.drag, sessionDidMove: session)
        try? await Task.sleep(for: .milliseconds(120))
        precondition(f.scroll.contentOffset.y == lowerOffset && f.model.hint == nil,
            "A contact outside the viewport must stop edge scrolling and clear the insertion hint")
        session.point.y = 10
        f.adapter.dragInteraction(f.drag, sessionDidMove: session)
        try? await Task.sleep(for: .milliseconds(250))
        precondition(f.scroll.contentOffset.y < lowerOffset, "The top edge must scroll back up")
        session.point.y = 100
        let drop = DropSession(session); drop.coordinateView = container; drop.point = session.point
        precondition(f.adapter.dropInteraction(f.drop, sessionDidUpdate: drop).operation == .move)
        let before = f.model.hint!.before
        var committed = false
        f.model.commit = { id, target in precondition(id == "A" && target == before); committed = true }
        f.adapter.dropInteraction(f.drop, performDrop: drop)
        let finishedOffset = f.scroll.contentOffset
        session.point.y = 190
        try? await Task.sleep(for: .milliseconds(120))
        precondition(committed && f.scroll.contentOffset == finishedOffset && !f.model.nativeDragging,
            "A drop must commit the scrolled insertion target and stop its frame callback")
        f.adapter.dragInteraction(f.drag, session: session, didEndWith: .move)
    }
    print("PASS: native row sessions preserve menu-to-drag ownership, one commit, cancellation, stale callbacks and source removal")
}

private final class Delegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        Task { @MainActor in await checks(); exit(0) }; return true
    }
}
@main struct NativeRowChecks {
    static func main() { UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(Delegate.self)) }
}
