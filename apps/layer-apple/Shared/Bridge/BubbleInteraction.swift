import SwiftUI

@MainActor final class BubbleInteraction {
    enum Admission { case ignore, observe, capture }
    private weak var bubbles: WorkspaceBubbles?
    private weak var store: EditorStore?
    @MainActor private final class Contact {
        let item: BubblePresentation
        let source: String
        let origin: CGPoint
        var last: CGPoint
        let hit: JSON
        var mode = "pending"
        var part = JSON()
        var wheelStarted = false
        var fingers: [UInt64: CGPoint]
        var held = false
        var pairEnded = false
        var pairLifted = false
        var pairOrigin = CGPoint.zero
        var spread: CGFloat = 1
        var scale: CGFloat = 1
        var midpoint = CGPoint.zero
        var nativeDrags = false
        init(item: BubblePresentation, hit: JSON, point: CGPoint, id: UInt64) {
            self.item = item; source = item.source; origin = point; last = point; self.hit = hit; fingers = [id: point]
        }
    }
    private struct Candidate {
        let grab: JSON
        let identity: String
        let bounds: CGRect
        let id: UInt64
        var last: CGPoint
        var moving = false
        var admissionPending = false
    }
    private var contact: Contact?
    private var candidate: Candidate?
    private weak var keyboardItem: BubblePresentation?
    private var lastRing: (id: UInt64, kind: String, point: CGPoint, time: TimeInterval)?
    private var suppressClick = false
    var hold: (() -> Void)?
    var claim: (@MainActor (UInt64) -> Void)?
    let colorTouches = ColorTouchSurface()
    private(set) var generation: UInt64 = 0
    var viewport = CGSize.zero
    var choiceSources: [String: BubbleChoice] = [:]
    var nativeRegions: [String: BubbleNativeRegion] = [:]
    var cancellations: [String: () -> Void] = [:]
    var active: Bool { contact != nil || candidate != nil }
    var captures: Bool { contact.map { $0.mode != "native" } ?? candidate?.moving ?? false }
    var holdMultiplier: Double { ["wheel", "intensity"].contains(contact?.hit.bubbleHit ?? "") ? 2 : 1 }
    var canHold: Bool { contact != nil && contact?.mode != "native" && contact?.mode != "pair" }
    var acceptsIntensityKeys: Bool {
        guard enabled, let keyboardItem else { return false }
        return bubbles?.items.contains { $0 === keyboardItem } == true && keyboardItem.model["color"]["intensity"]["enabled"].bool
    }
    @discardableResult func setIntensity(_ id: UInt64, stops: Double) -> Bool {
        guard enabled, let item = bubbles?.items.first(where: { $0.id == id }) else { return false }
        let data = item.model["color"]["intensity"]
        guard data["enabled"].bool else { return false }
        bubbles?.action(["type": "set_intensity", "id": id,
            "stops": min(data["range"][1].number, max(data["range"][0].number, stops))])
        return true
    }
    func intensityKey(_ key: String, id: UInt64? = nil, command: Bool = false, alt: Bool = false) -> Bool {
        guard !NativeTextContext.composing, !command, !alt,
            let id = id ?? keyboardItem?.id, let item = bubbles?.items.first(where: { $0.id == id }) else { return false }
        if key == "escape", active { cancel(); return true }
        if key == "Shift" { return acceptsIntensityKeys }
        let stops = item.model["color"]["intensity"]["stops"].number
        switch key {
        case "home": return setIntensity(id, stops: 0)
        case "arrowleft", "arrowdown": return setIntensity(id, stops: stops - 0.1)
        case "arrowright", "arrowup": return setIntensity(id, stops: stops + 0.1)
        default: return false
        }
    }
    func releaseIntensityKeys() { keyboardItem = nil; store?.focusCanvas?() }
    func hit(_ point: CGPoint) -> Bool { bubbles?.hit(point) != nil }
    func hidesPointer(at point: CGPoint, pen: Bool) -> Bool {
        (pen && hit(point)) || contact.map { !$0.item.view["stamp"].isNull } == true
    }
    func ownsSecondary(_ point: CGPoint) -> Bool {
        guard let c = contact else { return false }
        return hypot(point.x - c.item.placement.center.x, point.y - c.item.placement.center.y) <= c.item.model["reach"].number + 90
    }
    init(bubbles: WorkspaceBubbles, store: EditorStore?) {
        self.bubbles = bubbles; self.store = store
        colorTouches.send = { [weak store] in store?.input($0) }
    }
    func colorTouch(at point: CGPoint) -> Bool { enabled && bubbles?.hit(point)?.0.view["kind"].string == "color" }
    var enabled: Bool {
        guard let store, let bubbles else { return false }
        return store.failure == nil && store.workspaces?.presented != true
            && !store.workspace.hasPopover(excluding: bubbles.popup)
            && ["picker", "toolbar_prompt", "toolbar_manager", "preferences"].allSatisfy { store.snapshot[$0].isNull }
    }
    func consumeClick() -> Bool { defer { suppressClick = false }; return suppressClick }
    func validate() {
        if let contact {
            let current = bubbles?.items.contains { $0 === contact.item && $0.source == contact.source } == true
            if !enabled || !current {
                retire()
                if current { restore(contact) }
            }
        }
        if let candidate, !enabled || (!candidate.moving && sourceIdentity(candidate.grab) != candidate.identity) {
            retire()
            if candidate.moving { drag(candidate.grab, "cancel", candidate.last) }
        }
    }
    func retire() {
        if let item = contact?.item {
            if item.placement.pinch != 1 { item.placement.pinch = 1 }
            if !item.pressed.isNull { item.pressed = JSON() }
        }
        contact = nil; candidate = nil; hold?()
    }
    func cancel() {
        lastRing = nil
        validate()
        if contact == nil { keyboardItem = nil }
        if let contact { restore(contact); if contact.held { bubbles?.closeMenu() } }
        if let candidate, candidate.moving { drag(candidate.grab, "cancel", candidate.last) }
        retire()
    }
    private func sourceIdentity(_ grab: JSON) -> String {
        guard let store else { return "" }
        switch grab["kind"].string {
        case "header": return store.snapshot["header"]["items"].array.first { $0["id"].uint == grab["id"].uint }?["resolved_control"].stableKey ?? ""
        case "tile": return store.panel(grab["panel"].string)["tiles"].array.first { $0["id"].uint == grab["tile"].uint }?["resolved_control"].stableKey ?? ""
        case "variant": return choiceSources.values.contains { $0.grab.stableKey == grab.stableKey } ? grab.stableKey : ""
        default: return ""
        }
    }
    private func source(at point: CGPoint) -> (JSON, CGRect)? {
        guard let store, !store.snapshot["header"]["editing"].bool else { return nil }
        if let choice = choiceSources.values.first(where: { $0.bounds.contains(point) && !$0.grab.isNull && $0.grab["error"].isNull }) {
            let drawer = store.contentDrawers.items.values.first { $0.interactive && $0.geometry["placement"]["bounds"].rect.contains(point) }
            let group = store.snapshot["layout"]["groups"].array.first { $0["bounds"].rect.contains(point) }
            return (choice.grab, drawer?.geometry["placement"]["bounds"].rect ?? group?["bounds"].rect ?? choice.clip)
        }
        if let (_, source) = store.workspace.hitSource(at: point), source.surface == .tile,
            let item = try? JSON.decode(source.item), item["kind"].string == "tile",
            let tile = store.panel(item["panel"].string)["tiles"].array.first(where: { $0["id"].uint == item["tile"].uint }), tile["tears_bubble"].bool {
            let group = store.snapshot["layout"]["groups"].array.first { $0["panels"].array.contains { $0.string == item["panel"].string } }
            return (item, group?["bounds"].rect ?? source.bounds)
        }
        if !store.snapshot["chrome_hidden"].bool,
            let entry = store.snapshot["header"]["items"].array.first(where: { entry in
                entry["tears_bubble"].bool && store.header.geometry["items"].array.contains {
                    $0["id"].uint == entry["id"].uint && $0["bounds"].rect.contains(point)
                }
            }) {
            return (JSON(["kind": "header", "id": entry["id"].raw]), CGRect(x: 0, y: 0, width: viewport.width, height: store.header.size["height"].number))
        }
        return nil
    }
    func down(_ point: CGPoint, id: UInt64, device: ReorderDevice, clicks: Int, doubleTime: TimeInterval, time: TimeInterval) -> Admission {
        let previousRing = lastRing
        lastRing = nil
        validate()
        if contact == nil { keyboardItem = nil }
        guard enabled, let bubbles else { return .ignore }
        generation &+= 1
        if let c = contact {
            if c.mode == "native" && nativeRegions.values.contains(where: { $0.owner == c.item.id && $0.bounds.contains(point) }) { return .ignore }
            let center = c.item.placement.center
            guard device == .touch, c.fingers.count == 1,
                hypot(point.x - center.x, point.y - center.y) <= c.item.model["reach"].number + 90 else { return .ignore }
            hold?(); restore(c); bubbles.closeMenu()
            c.fingers[id] = point
            let pair = Array(c.fingers.values)
            c.midpoint = CGPoint(x: (pair[0].x + pair[1].x) / 2, y: (pair[0].y + pair[1].y) / 2)
            c.spread = max(1, hypot(pair[0].x - pair[1].x, pair[0].y - pair[1].y))
            c.pairOrigin = c.midpoint; c.mode = "pair"
            return .capture
        }
        guard bubbles.menu.isNull else { return .ignore }
        bubbles.closeMenu()
        suppressClick = false
        guard let (item, hit) = bubbles.hit(point) else {
            if let (grab, bounds) = source(at: point) {
                candidate = Candidate(grab: grab, identity: sourceIdentity(grab), bounds: bounds, id: id, last: point)
                return .observe
            }
            return .ignore
        }
        let c = Contact(item: item, hit: hit, point: point, id: id); contact = c
        if hit.bubbleHit == "intensity", item.model["color"]["intensity"]["enabled"].bool { keyboardItem = item }
        item.pressed = hit
        if hit.bubbleHit == "card", let region = nativeRegions.values.filter({ $0.owner == item.id && $0.bounds.contains(point) }).max(by: {
            $0.bounds.width * $0.bounds.height < $1.bounds.width * $1.bounds.height
        }) {
            c.mode = "native"; c.nativeDrags = region.drags; return .observe
        }
        if hit.bubbleHit == "pill" { c.mode = "pill"; return .capture }
        if hit.bubbleHit == "wheel" {
            let size = item.view["diameter"].number - 14, relative = local(c, point)
            c.part = ColorUI.resolve(["type": "wheel_hit", "size": size,
                "point": [relative.x + size / 2, relative.y + size / 2], "shape": store?.colorPanel["shape"].raw ?? "circle"])
            c.mode = "wheel"
            if !c.part.isNull { send(c, "wheel", "down", point) }
        } else if hit.bubbleHit == "ring" || hit.bubbleHit == "intensity" {
            let kind = hit.bubbleHit
            c.mode = kind
            guard kind != "intensity" || item.model["color"]["intensity"]["enabled"].bool else { return .capture }
            let repeated = clicks == 2 || (device != .mouse && previousRing.map {
                $0.id == item.id && $0.kind == kind && time - $0.time <= doubleTime && hypot(point.x - $0.point.x, point.y - $0.point.y) <= 8
            } == true)
            lastRing = repeated ? nil : (item.id, kind, point, time)
            c.mode = repeated ? "reset" : kind
            if repeated {
                if kind == "intensity" { bubbles.action(["type": "set_intensity", "id": item.id, "stops": 0]) }
                else { bubbles.action(["type": "reset_ring", "id": item.id, "position": [local(c, point).x, local(c, point).y]]) }
            } else { send(c, kind, "down", point) }
        } else if hit.bubbleHit == "bead" || hit.bubbleHit == "item" {
            c.mode = hit.bubbleHit == "bead" ? "beads" : "choose"; send(c, c.mode, "down", point)
        }
        return .capture
    }
    func move(_ point: CGPoint, id: UInt64) -> Bool {
        validate()
        if var s = candidate, s.id == id {
            s.last = point
            if !s.moving && !s.bounds.contains(point) && store?.workspace.input.contact.held != true {
                candidate = s
                requestAdmission()
                return false
            }
            if s.moving { drag(s.grab, "move", point) }
            candidate = s; return s.moving
        }
        guard let c = contact, c.fingers[id] != nil else { return false }
        c.last = point; c.fingers[id] = point
        if c.mode == "pair" {
            guard c.fingers.count == 2, !c.pairEnded else { return true }
            let pair = Array(c.fingers.values)
            c.midpoint = CGPoint(x: (pair[0].x + pair[1].x) / 2, y: (pair[0].y + pair[1].y) / 2)
            let spread = hypot(pair[0].x - pair[1].x, pair[0].y - pair[1].y)
            if !c.pairLifted {
                guard hypot(c.midpoint.x - c.pairOrigin.x, c.midpoint.y - c.pairOrigin.y) > 8 || abs(spread - c.spread) > 8 else { return true }
                c.pairLifted = true; drag(grab(c.item.id), "down", c.pairOrigin)
            }
            if c.item.view["kind"].string == "color" {
                let size = c.item.view["diameter"].number
                c.scale = BubbleUI.resolve(["type": "pinched_size", "size": size, "scale": spread / c.spread]).number / size
                c.item.placement.pinch = c.scale
            }
            drag(grab(c.item.id), "move", c.midpoint); return true
        }
        let moved = hypot(point.x - c.origin.x, point.y - c.origin.y) > 8
        if moved { hold?(); lastRing = nil }
        if c.mode == "native" {
            guard moved && !c.nativeDrags else { return false }
            restore(c)
        }
        if c.held {
            guard moved else { return true }
            bubbles?.closeMenu(); c.held = false
            c.mode = ["wheel", "intensity"].contains(c.hit.bubbleHit) ? c.hit.bubbleHit : "pending"
        }
        if c.mode == "wheel" {
            let relative = local(c, point)
            if BubbleUI.resolve(["type": "color_drag_out", "size": c.item.view["diameter"].raw, "point": [relative.x, relative.y]]).bool {
                restore(c); c.mode = "rim"; drag(grab(c.item.id, rim: true), "down", point)
            } else if !c.part.isNull { send(c, "wheel", "move", point) }
        } else if c.mode == "pending" && moved {
            c.mode = "moving"; drag(grab(c.item.id), "down", c.origin)
        }
        if c.mode == "moving" || c.mode == "rim" { drag(grab(c.item.id, rim: c.mode == "rim"), "move", point) }
        else if ["ring", "intensity", "choose", "beads"].contains(c.mode) { send(c, c.mode, "move", point) }
        c.item.hover = c.item.hit(point)
        return true
    }
    private func requestAdmission() {
        guard var s = candidate, !s.moving, !s.admissionPending, let store else { return }
        s.admissionPending = true
        candidate = s
        let point = s.last, id = s.id, token = generation, queryViewport = viewport
        store.query(["type": "can_take_bubble", "grab": s.grab.raw, "position": [point.x, point.y],
            "viewport": [queryViewport.width, queryViewport.height]]) { [weak self] reply in
            guard let self, self.generation == token, var current = self.candidate, current.id == id else { return }
            current.admissionPending = false
            self.candidate = current
            self.validate()
            guard let candidate = self.candidate, candidate.id == id, !candidate.moving,
                !candidate.bounds.contains(candidate.last), self.store?.workspace.input.contact.held != true else { return }
            if candidate.last != point || self.viewport != queryViewport { self.requestAdmission(); return }
            guard reply.bool else { return }
            current = candidate; current.moving = true; self.candidate = current
            self.store?.workspace.input.relinquishToBubble(); self.store?.header.cancel()
            self.suppressClick = true
            self.claim?(id)
            self.drag(current.grab, "down", point)
        }
    }
    func up(_ point: CGPoint, id: UInt64, cancelled: Bool) -> Bool {
        validate(); hold?()
        if let s = candidate, s.id == id {
            candidate = nil
            if s.moving { drag(s.grab, cancelled ? "cancel" : "up", point) }
            return s.moving
        }
        guard let c = contact, c.fingers[id] != nil else { return false }
        let captured = c.mode != "native"
        if cancelled { lastRing = nil; restore(c); c.mode = "retired"; c.pairEnded = true }
        else if c.mode == "pair" && !c.pairEnded {
            c.pairEnded = true; c.item.placement.pinch = 1
            if c.pairLifted {
                drag(grab(c.item.id), "up", c.midpoint)
                if c.item.view["kind"].string == "color" && abs(c.scale - 1) > 0.01 { bubbles?.action(["type": "resize", "id": c.item.id, "scale": c.scale]) }
            }
        } else if !c.held {
            switch c.mode {
            case "moving", "rim": drag(grab(c.item.id, rim: c.mode == "rim"), "up", point)
            case "wheel": if !c.part.isNull { send(c, "wheel", "up", point) }
            case "ring", "intensity", "beads", "choose": send(c, c.mode, "up", point)
            case "pending": bubbles?.action(["type": "tap", "id": c.item.id, "position": [local(c, point).x, local(c, point).y]])
            case "pill": pill(c, point)
            default: break
            }
        }
        c.fingers.removeValue(forKey: id)
        if c.fingers.isEmpty { c.item.pressed = JSON(); contact = nil }
        suppressClick = captured; return captured
    }
    func recognizeHold() {
        validate()
        guard let c = contact, canHold else { return }
        store?.edgeReach.cancelPending()
        lastRing = nil; restore(c); c.held = true; bubbles?.showMenu(c.item, at: c.origin)
    }
    @discardableResult func context(at point: CGPoint) -> Bool {
        guard enabled, let (item, hit) = bubbles?.hit(point) else { return false }
        if hit.bubbleHit == "card" && nativeRegions.values.contains(where: { $0.owner == item.id && $0.bounds.contains(point) }) { return false }
        cancel(); bubbles?.showMenu(item, at: point); return true
    }
    func hover(_ point: CGPoint?) {
        let hit = point.flatMap { bubbles?.hit($0) }
        for item in bubbles?.items ?? [] {
            let next = hit?.0 === item ? hit!.1 : JSON()
            if item.hover.stableKey != next.stableKey { item.hover = next }
        }
    }
    private func pill(_ c: Contact, _ point: CGPoint) {
        let x = local(c, point).x, width = c.item.model["pill"]["width"].number
        if x < -width / 2 + 30 || x > width / 2 - 30 { bubbles?.action(["type": "palette", "id": c.item.id, "step": x < 0 ? -1 : 1]) }
        else { bubbles?.showMenu(c.item, at: point, palettes: true) }
    }
    private func restore(_ c: Contact) {
        c.item.placement.pinch = 1
        if c.mode == "native" { for cancel in Array(cancellations.values) { cancel() } }
        else if c.mode == "pair", !c.pairEnded, c.pairLifted { drag(grab(c.item.id), "cancel", c.midpoint) }
        else if c.mode == "moving" || c.mode == "rim" { drag(grab(c.item.id, rim: c.mode == "rim"), "cancel", c.last) }
        else if ["ring", "intensity", "beads", "choose"].contains(c.mode) || (c.mode == "wheel" && !c.part.isNull) { send(c, c.mode, "cancel", c.last) }
        c.mode = "pending"
    }
    private func grab(_ id: UInt64, rim: Bool = false) -> JSON { JSON(["kind": rim ? "rim" : "bubble", "id": id]) }
    private func local(_ c: Contact, _ point: CGPoint) -> CGPoint { CGPoint(x: point.x - c.item.placement.center.x, y: point.y - c.item.placement.center.y) }
    private func drag(_ grab: JSON, _ phase: String, _ point: CGPoint) {
        bubbles?.action(["type": "drag", "grab": grab.raw, "phase": phase, "position": [point.x, point.y], "viewport": [viewport.width, viewport.height]])
    }
    private func send(_ c: Contact, _ kind: String, _ phase: String, _ point: CGPoint) {
        let color = kind == "wheel" || kind == "intensity"
        if color, (phase == "up" || phase == "cancel") && !c.wheelStarted { return }
        let phase = color && phase == "move" && !c.wheelStarted ? "down" : phase
        if color { c.wheelStarted = phase == "down" || phase == "move" }
        let p = local(c, point)
        var value: [String: Any] = ["type": kind, "id": c.item.id, "phase": phase, "position": [p.x, p.y]]
        if kind == "wheel" { value["part"] = c.part.raw }
        bubbles?.action(value)
    }
}

struct BubbleChoice: Equatable {
    let action: String
    let bounds: CGRect
    let clip: CGRect
    var grab: JSON { BubbleUI.resolve(["type": "choice", "action": ((try? JSON.decode(action)) ?? JSON()).raw]) }
}
struct BubbleChoices: PreferenceKey {
    static var defaultValue: [String: BubbleChoice] { [:] }
    static func reduce(value: inout [String: BubbleChoice], nextValue: () -> [String: BubbleChoice]) { value.merge(nextValue(), uniquingKeysWith: { _, b in b }) }
}
struct BubbleChoiceSource: ViewModifier {
    let action: JSON
    @Environment(\.workspaceClip) private var clip
    @State private var id = UUID().uuidString
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
                let bounds = geometry.frame(in: .named("editor-workspace"))
                Color.clear.preference(key: BubbleChoices.self, value: [id: BubbleChoice(action: action.stableKey,
                    bounds: bounds.intersection(clip), clip: clip.isInfinite ? bounds : clip)])
            })
    }
}

struct BubbleNativeRegion: Equatable {
    let owner: UInt64
    let bounds: CGRect
    let drags: Bool
}
struct BubbleNativeRegions: PreferenceKey {
    static var defaultValue: [String: BubbleNativeRegion] { [:] }
    static func reduce(value: inout [String: BubbleNativeRegion], nextValue: () -> [String: BubbleNativeRegion]) { value.merge(nextValue(), uniquingKeysWith: { _, b in b }) }
}
struct BubbleNativeControl: ViewModifier {
    var drags = true
    var viewport: CGRect? = nil
    var cancel: () -> Void = {}
    @Environment(\.bubbleEdits) private var edits
    @Environment(\.bubbleCardID) private var owner
    @State private var id = UUID().uuidString
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
            let bounds = geometry.frame(in: .named("editor-workspace"))
            let region = viewport.map { CGRect(x: $0.minX, y: bounds.minY, width: $0.width, height: bounds.height).intersection($0) } ?? bounds
            Color.clear.preference(key: BubbleNativeRegions.self,
                value: edits == nil ? [:] : [id: BubbleNativeRegion(owner: owner, bounds: region, drags: drags)])
        })
            .onAppear { edits?.cancellations[id] = cancel }
            .onDisappear { edits?.cancellations.removeValue(forKey: id) }
    }
}
private struct BubbleEditsKey: EnvironmentKey { static let defaultValue: BubbleInteraction? = nil }
private struct BubbleCardIDKey: EnvironmentKey { static let defaultValue: UInt64 = 0 }
extension EnvironmentValues {
    var bubbleEdits: BubbleInteraction? { get { self[BubbleEditsKey.self] } set { self[BubbleEditsKey.self] = newValue } }
    var bubbleCardID: UInt64 { get { self[BubbleCardIDKey.self] } set { self[BubbleCardIDKey.self] = newValue } }
}
