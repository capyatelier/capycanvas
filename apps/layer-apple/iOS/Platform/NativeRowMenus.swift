import UIKit

/// UIKit owns the held contact while a native menu transitions into a drag.
/// Attach to the list's scroll view so SwiftUI row geometry stays in one tree.
@MainActor final class NativeRowMenuCoordinator: NSObject, UIContextMenuInteractionDelegate,
    UIDragInteractionDelegate, UIDropInteractionDelegate {
    private weak var input: ReorderInputView?
    private weak var scroll: UIScrollView?
    private lazy var contextMenu = UIContextMenuInteraction(delegate: self)
    private lazy var drag = UIDragInteraction(delegate: self)
    private lazy var drop = UIDropInteraction(delegate: self)
    private var source: NativeReorderMenu?
    private var menuContact: RowContact?
    private var ticket = UUID()
    private var active: RowDrag?
    private var link: CADisplayLink?
    private(set) var isMenuVisible = false
    var buttons: UIEvent.ButtonMask = []
    private var model: (any NativeReorderModel)? { input?.model }

    init(input: ReorderInputView, scroll: UIScrollView) {
        self.input = input; self.scroll = scroll; super.init()
        drag.isEnabled = true; drag.allowsSimultaneousRecognitionDuringLift = true
        scroll.addInteraction(drag); scroll.addInteraction(contextMenu); scroll.addInteraction(drop)
    }
    func attached(to scroll: UIScrollView) -> Bool { self.scroll === scroll }
    func detach() {
        cancel(deferPublication: true)
        scroll?.removeInteraction(contextMenu); scroll?.removeInteraction(drag); scroll?.removeInteraction(drop)
        scroll = nil; input = nil
    }
    func cancel(deferPublication: Bool = false) {
        let pending = active, held = menuContact
        stopTracking()
        ticket = UUID(); source = nil; menuContact = nil; active = nil
        isMenuVisible = false; contextMenu.dismissMenu()
        pending?.contact.cancel(deferPublication: deferPublication)
        held?.cancel(deferPublication: deferPublication)
    }
    func validate() {
        guard let model, model.enabled else { cancel(); return }
        if let active, !active.contact.matches(model) || !model.contact.validate() { cancel(); return }
        if let source, active == nil,
           model.nativeMenu(at: CGPoint(x: source.bounds.midX, y: source.bounds.midY))?.id != source.id { cancel() }
    }
    func ownsPickup(_ target: ReorderTarget, device: ReorderDevice) -> Bool {
        device != .mouse && target.surface == .row
    }
    private func point(_ point: CGPoint) -> CGPoint? {
        guard let input, let scroll else { return nil }; return input.convert(point, from: scroll)
    }
    private func preview(_ source: NativeReorderMenu) -> (UIView, CGRect)? {
        guard let input, let scroll else { return nil }
        let rect = scroll.convert(source.bounds, from: input).intersection(scroll.bounds)
        guard !rect.isEmpty, let snapshot = scroll.resizableSnapshotView(from: rect,
            afterScreenUpdates: false, withCapInsets: .zero) else { return nil }
        snapshot.frame = CGRect(origin: .zero, size: rect.size)
        // Rows have translucent selection fills. Their lifted copy needs an
        // opaque backing or the original label shows through at a second offset.
        let backing = UIView(frame: snapshot.frame)
        backing.backgroundColor = UIColor.secondarySystemBackground.resolvedColor(with: input.traitCollection)
        backing.addSubview(snapshot)
        return (backing, rect)
    }
    private func menuPreview() -> UITargetedPreview? {
        guard let source, let scroll, let (view, rect) = preview(source) else { return nil }
        let parameters = UIPreviewParameters(); parameters.backgroundColor = .clear
        parameters.visiblePath = UIBezierPath(roundedRect: view.bounds, cornerRadius: 6)
        return UITargetedPreview(view: view, parameters: parameters,
            target: UIPreviewTarget(container: scroll, center: CGPoint(x: rect.midX, y: rect.midY)))
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction,
        configurationForMenuAtLocation location: CGPoint) -> UIContextMenuConfiguration? {
        guard let model, let point = point(location),
              !(model.contact.device == .mouse && buttons.contains(.primary)),
              model.contact.target?.surface != .handle,
              let source = model.nativeMenu(at: point) else { return nil }
        self.source = source
        menuContact = model.contact.target?.id == source.id && model.contact.device != .mouse
            ? RowContact(model: model, id: source.id) : nil
        ticket = UUID(); let current = ticket
        let configuration = UIContextMenuConfiguration(identifier: current.uuidString as NSString,
            previewProvider: nil) { [weak self] _ in
                guard let self, ticket == current else { return nil }
                return UIMenu(children: [UIDeferredMenuElement.uncached { [weak self] completion in
                    Task { @MainActor in
                        guard let self, self.ticket == current else { completion([]); return }
                        source.load { [weak self] menu in
                            guard let self, self.ticket == current else { completion([]); return }
                            completion(menu?.nativeMenu().children ?? [])
                        }
                    }
                }])
            }
        configuration.preferredMenuElementOrder = .fixed
        return configuration
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, configuration: UIContextMenuConfiguration,
        highlightPreviewForItemWithIdentifier identifier: any NSCopying) -> UITargetedPreview? {
        guard configuration.identifier as? String == ticket.uuidString else { return nil }
        return menuPreview()
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, configuration: UIContextMenuConfiguration,
        dismissalPreviewForItemWithIdentifier identifier: any NSCopying) -> UITargetedPreview? {
        guard configuration.identifier as? String == ticket.uuidString else { return nil }
        return menuPreview()
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, willDisplayMenuFor configuration: UIContextMenuConfiguration,
        animator: (any UIContextMenuInteractionAnimating)?) {
        guard configuration.identifier as? String == ticket.uuidString else { return }
        isMenuVisible = true
        if menuContact?.matches(model) == true { model?.contact.recognizeHold(openContext: false) }
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, willEndFor configuration: UIContextMenuConfiguration,
        animator: (any UIContextMenuInteractionAnimating)?) {
        guard configuration.identifier as? String == ticket.uuidString else { return }
        isMenuVisible = false
        // Items may be lifting before sessionWillBegin. Preserve that contact
        // until UIKit either begins the drag or reports a cancelled lift.
        let current = ticket
        let finish = { [weak self] in
            guard let self, ticket == current else { return }
            if active?.contact.matches(model) != true { menuContact?.cancel() }
            menuContact = nil; source = nil
        }
        if let animator { animator.addCompletion(finish) } else { finish() }
    }
    func dragInteraction(_ interaction: UIDragInteraction, itemsForBeginning session: UIDragSession) -> [UIDragItem] {
        guard let input, let model, model.enabled, model.contact.device != .mouse,
              let target = model.contact.target, target.surface == .row, target.canDrag,
              let source = model.nativeMenu(at: session.location(in: input)), source.id == target.id else { return [] }
        guard active == nil else { return [] }
        let active = RowDrag(model: model, source: source, session: session); self.active = active
        let item = UIDragItem(itemProvider: NSItemProvider(object: source.id as NSString)); item.localObject = active
        return [item]
    }
    func dragInteraction(_ interaction: UIDragInteraction, previewForLifting item: UIDragItem,
        session: UIDragSession) -> UITargetedDragPreview? {
        guard let active = rowDrag(session), item.localObject as? RowDrag === active, let scroll,
              let (view, rect) = preview(active.source) else { return nil }
        let parameters = UIDragPreviewParameters(); parameters.backgroundColor = .clear
        parameters.visiblePath = UIBezierPath(roundedRect: view.bounds, cornerRadius: 6)
        return UITargetedDragPreview(view: view, parameters: parameters,
            target: UIDragPreviewTarget(container: scroll, center: CGPoint(x: rect.midX, y: rect.midY)))
    }
    func dragInteraction(_ interaction: UIDragInteraction, willAnimateLiftWith animator: any UIDragAnimating,
        session: any UIDragSession) {
        guard let active = rowDrag(session) else { return }
        animator.addCompletion { [weak self, weak active] position in
            guard let self, let active, self.active === active, position == .start else { return }
            self.active = nil
            // A stationary hold can finish its lift while retaining the menu.
            if isMenuVisible { return }
            active.contact.cancel()
        }
    }
    func dragInteraction(_ interaction: UIDragInteraction, sessionWillBegin session: UIDragSession) {
        guard let active = rowDrag(session), let model, let input,
              active.contact.matches(model), !active.started else { return }
        active.started = true; model.nativeDragChanged(true)
        model.contact.recognizeHold(openContext: false)
        guard input.moveReorder(session.location(in: input)) else { cancel(); return }
        updateTracking(session)
        contextMenu.dismissMenu()
    }
    func dragInteraction(_ interaction: UIDragInteraction, sessionDidMove session: UIDragSession) {
        guard let active = rowDrag(session), let model, let input,
              active.started, !active.committed, active.contact.matches(model) else { return }
        _ = input.moveReorder(session.location(in: input))
        updateTracking(session)
    }
    func dragInteraction(_ interaction: UIDragInteraction, session: any UIDragSession,
        didEndWith operation: UIDropOperation) {
        guard let active = rowDrag(session) else { return }
        stopTracking()
        self.active = nil
        active.contact.cancel()
    }
    private func stopTracking() { link?.invalidate(); link = nil }
    private func updateTracking(_ session: any UIDragSession) {
        guard let input, input.needsReorderScrolling(at: session.location(in: input)) else { stopTracking(); return }
        guard link == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(track))
        link.add(to: .main, forMode: .common); self.link = link
    }
    @objc private func track() {
        guard let active, let session = active.session, rowDrag(session) === active,
              active.started, !active.committed, let model, let input,
              active.contact.matches(model) else { stopTracking(); return }
        guard model.enabled, model.contact.validate() else { cancel(); return }
        if !input.trackReorder(location: { session.location(in: input) }) { stopTracking() }
    }
    private func rowDrag(_ session: any UIDragSession) -> RowDrag? {
        guard let active, active.session === session,
              session.items.first?.localObject as? RowDrag === active else { return nil }
        return active
    }
    private func rowDrag(_ session: any UIDropSession, requiresStarted: Bool = true) -> RowDrag? {
        guard let local = session.localDragSession, let active = rowDrag(local),
              session.items.first?.localObject as? RowDrag === active,
              (!requiresStarted || active.started), !active.committed, active.contact.matches(model) else { return nil }
        return active
    }
    func dragInteraction(_ interaction: UIDragInteraction, sessionIsRestrictedToDraggingApplication session: UIDragSession) -> Bool { true }
    func dropInteraction(_ interaction: UIDropInteraction, canHandle session: UIDropSession) -> Bool {
        // Destination interest is separate from starting or committing a move.
        rowDrag(session, requiresStarted: false) != nil
    }
    func dropInteraction(_ interaction: UIDropInteraction, sessionDidUpdate session: UIDropSession) -> UIDropProposal {
        guard let active = rowDrag(session), let model, let input,
              model.contact.validate() else { return UIDropProposal(operation: .cancel) }
        _ = input.moveReorder(session.location(in: input))
        if let local = active.session { updateTracking(local) }
        return UIDropProposal(operation: .move)
    }
    func dropInteraction(_ interaction: UIDropInteraction, performDrop session: UIDropSession) {
        guard let active = rowDrag(session), let model, let input else { return }
        stopTracking()
        _ = input.moveReorder(session.location(in: input))
        active.committed = true; model.contact.release(at: session.location(in: input))
        model.nativeDragChanged(false)
    }
    @MainActor private final class RowContact {
        weak var model: (any NativeReorderModel)?
        let id: String
        let generation: UInt64
        init(model: any NativeReorderModel, id: String) {
            self.model = model; self.id = id; generation = model.contact.generation
        }
        func matches(_ current: (any NativeReorderModel)?) -> Bool {
            guard let current, model === current else { return false }
            return current.contact.generation == generation && current.contact.target?.id == id
        }
        func cancel(deferPublication: Bool = false) {
            if deferPublication {
                // SwiftUI may still own its graph while detaching this source.
                // Retire native callbacks immediately; publish after teardown.
                DispatchQueue.main.async { [self] in cancel() }
                return
            }
            // Source removal may already have cleared target. Still retire this
            // generation's native feedback, while leaving a newer contact alone.
            guard let model, model.contact.generation == generation else { return }
            model.contact.cancel(); model.nativeDragChanged(false)
        }
    }
    @MainActor private final class RowDrag {
        let contact: RowContact
        let source: NativeReorderMenu
        weak var session: (any UIDragSession)?
        var started = false
        var committed = false
        init(model: any NativeReorderModel, source: NativeReorderMenu, session: any UIDragSession) {
            contact = RowContact(model: model, id: source.id); self.source = source; self.session = session
        }
    }
}
