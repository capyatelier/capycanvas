import SwiftUI
import Combine

@MainActor final class PhoneMenuPresentation: ObservableObject, NativeReorderModel {
    let contact = ReorderContact()
    let position = EditorScrollPosition()
    var viewport = CGRect.zero
    let drawer: ContentDrawerPresentation
    var model: JSON { drawer.model }
    var frame: JSON { drawer.geometry }
    @Published private(set) var progress: CGFloat = 0
    @Published private(set) var swiping = false
    @Published var focused: String?
    @Published private(set) var pressed: String?
    private(set) var layout = JSON()
    private(set) var route = JSON()
    private(set) var target = JSON()
    private(set) var geometry = JSON()
    private weak var store: EditorStore?
    private var tracker = JSON()
    private var frames: [String: CGRect] = [:]
    private var query = UUID()
    private var generation: UInt64 = 0
    private var observation: AnyCancellable?
    private var settle: Task<Void, Never>?
    private(set) var open = false
    @Published private var natural: CGFloat = 0
    private var rootHeight: CGFloat = 0
    @Published private(set) var rowHeight: CGFloat = 0
    var rtl = false
    var reduceMotion = false
    var enabled: Bool { open && !model.isNull && settle == nil && store?.failure == nil }
    var scrollPosition: EditorScrollPosition? { position }
    var page: JSON { model["pages"].array.first { $0["id"].stableKey == route.stableKey } ?? JSON() }
    var parent: JSON { model["pages"].array.first { $0["id"].stableKey == page["parent"].stableKey } ?? JSON() }
    var root: JSON { model["pages"].array.first { $0["parent"].isNull } ?? JSON() }
    var listWidth: CGFloat {
        guard !layout.isNull, natural > 0, rowHeight > 0 else { return 0 }
        return PhoneUI.resolve(["kind": "menu_width", "layout": layout.raw, "minimum": 0,
            "natural": natural, "row_height": rowHeight]).number
    }
    init(store: EditorStore) {
        self.store = store; drawer = store.contentDrawers.menu
        observation = drawer.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
    }
    private func structure(_ value: JSON) -> String {
        JSON(value["pages"].array.map { page in [page["id"].raw, page["parent"].raw, page["title"].raw, page["checks"].raw,
            page["sections"].array.map { section in section.array.map { row in
                [row["is_heading"].bool ? row["label"].raw : false, row["target"].raw, !row["close"].isNull] as [Any]
            } }] as [Any] }).stableKey
    }
    func refresh(layout: JSON, phone: JSON) {
        let nextRoute = phone["menu_route"], nextOpen = phone["menu"].bool && !layout.isNull
        let changed = self.layout.stableKey != layout.stableKey || route.stableKey != nextRoute.stableKey || open != nextOpen
        self.layout = layout; route = nextRoute; open = nextOpen
        if changed { cancel(); focused = nil; position.scrollTo(y: 0) }
        let request = UUID(); query = request
        guard nextOpen, let store else { if !model.isNull { present(closing: true) }; return }
        store.query(["type": "document_tabs", "op": "phone_menu"]) { [weak self] value in
            guard let self, query == request, open, !value.isNull else { return }
            if model.stableKey != value.stableKey {
                if structure(model) != structure(value) { cancel(); focused = nil }
            }
            drawer.present(value.replacing("anchor", with: JSON(["kind": "phone", "anchor": ["kind": "capy"]])), placement: target)
            measure()
        }
    }
    func measured(natural: CGFloat? = nil, height: CGFloat? = nil, row: CGFloat? = nil) {
        if let natural { self.natural = natural }
        if let height { rootHeight = height }
        if let row { rowHeight = row }
        measure()
    }
    private func measure() {
        guard open, listWidth > 0, rootHeight > 0, let store else { return }
        let next = PhoneUI.resolve(["kind": "menu_placement", "layout": layout.raw,
            "viewport": store.snapshot["layout"]["viewport"].raw, "width": listWidth,
            "height": rootHeight, "row_height": rowHeight])
        guard !next.isNull, next["error"].isNull else { return }
        if next.stableKey != target.stableKey { cancel(); target = next }
        geometry = PhoneUI.resolve(["kind": "menu_geometry", "layout": layout.raw,
            "placement": next.raw, "row_height": rowHeight])
        present(closing: false)
    }
    private func present(closing: Bool) {
        drawer.reduceMotion = reduceMotion
        drawer.present(closing ? JSON() : model, placement: target)
    }
    func disappear() {
        query = UUID(); cancel()
        store?.contentDrawers.remove(drawer.id)
    }
    private func input(_ value: [String: Any]) -> JSON {
        let reply = PhoneUI.resolve(["kind": "menu_input", "contact": tracker.isNull ? JSON([String: Any]()).raw : tracker.raw,
            "route": route.raw, "can_back": !page["parent"].isNull, "input": value])
        tracker = reply["contact"]
        return reply["reply"]
    }
    private var time: UInt32 { UInt32(truncatingIfNeeded: DispatchTime.now().uptimeNanoseconds / 1_000_000) }
    static func hit(_ kind: String, _ position: Any? = nil) -> JSON {
        position.map { JSON(["kind": kind, "position": $0]) } ?? JSON(["kind": kind])
    }
    var order: [String] {
        var values: [JSON] = page["parent"].isNull ? [] : [Self.hit("back")]
        for (section, rows) in page["sections"].array.enumerated() {
            for (index, row) in rows.array.enumerated() where row["enabled"].bool && !row["is_heading"].bool {
                values.append(Self.hit("row", [section, index]))
                if !row["close"].isNull { values.append(Self.hit("close", [section, index])) }
            }
        }
        values += model["strip"].array.indices.filter { model["strip"][$0]["enabled"].bool }.map { Self.hit("strip", $0) }
        return values.map(\.stableKey)
    }
    func allocated(_ hit: JSON, _ bounds: CGRect) { frames[hit.stableKey] = bounds }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard enabled, frame["placement"]["bounds"].rect.contains(point) else { return nil }
        let clip = geometry["list"].rect.offsetBy(dx: target["bounds"]["x"].number, dy: target["bounds"]["y"].number)
        let inList = clip.contains(point)
        let key = order.last(where: {
            guard let bounds = frames[$0], let hit = try? JSON.decode($0) else { return false }
            return (hit["kind"].string == "strip" ? bounds : bounds.intersection(clip)).contains(point)
        })
        guard key != nil || inList else { return nil }
        let hit = key.flatMap { try? JSON.decode($0) }
        return ReorderTarget(id: key ?? "phone-menu-list", surface: .row, canDrag: false,
            valid: { [weak self] _ in self?.enabled == true && (key == nil || self?.order.contains(key!) == true) },
            press: { [weak self] in
                guard let self else { return }
                if let hit { pressed = key; _ = input(["kind": "press", "hit": hit.raw]) }
                if inList { _ = input(["kind": "begin_swipe", "time_ms": time]) }
            }, begin: { _ in }, move: { _ in }, finish: { _ in }, cancel: {})
    }
    func acceptsContext(at point: CGPoint) -> Bool { false }
    func context(at point: CGPoint) {}
    func recognizeHold() {}
    func cancelPress() { _ = input(["kind": "cancel_press"]); pressed = nil }
    func cancel() {
        generation &+= 1; settle?.cancel(); settle = nil; contact.cancel()
        tracker = JSON(); pressed = nil; progress = 0; swiping = false
    }
    func beginSwipe(at point: CGPoint) -> Bool {
        guard enabled else { return false }
        moveSwipe(to: point)
        return swiping
    }
    func moveSwipe(to point: CGPoint) {
        let swipe = input(["kind": "swipe", "delta": [point.x - contact.origin.x, point.y - contact.origin.y],
            "width": geometry["list"]["width"].number, "past_slop": true, "rtl": rtl, "time_ms": time])["swipe"]
        if swipe["kind"].string != "pending" { pressed = nil }
        if swipe["kind"].string == "back" { swiping = true; progress = swipe["progress"].number }
    }
    func finishSwipe(cancelled: Bool) {
        guard swiping else { return }
        guard !cancelled else { cancel(); return }
        let result = input(["kind": "end_swipe", "width": geometry["list"]["width"].number,
            "rtl": rtl, "time_ms": time])["settle"]
        let action = result["action"], duration = reduceMotion ? 0 : result["duration_ms"].number / 1000
        let generation = self.generation
        withAnimation(.timingCurve(0.215, 0.61, 0.355, 1, duration: duration)) { progress = action.isNull ? 0 : 1 }
        settle = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(duration)) } catch { return }
            guard let self, self.generation == generation else { return }
            if !action.isNull { send(action) }
            if action.isNull { settle = nil; swiping = false; progress = 0 }
        }
    }
    func activate(_ hit: JSON, semantic: Bool = false) {
        guard enabled, order.contains(hit.stableKey) else { return }
        if semantic { _ = input(["kind": "press", "hit": hit.raw]) }
        guard input(["kind": "release", "hit": hit.raw, "inside": true])["accepted"].bool else { return }
        contact.cancel(); pressed = nil
        switch hit["kind"].string {
        case "back": send(JSON(["kind": "menu_back", "route": route.raw]))
        case "strip": store?.dispatch(model["strip"][Int(hit["position"].number)]["action"])
        default:
            let row = page["sections"][Int(hit["position"][0].number)][Int(hit["position"][1].number)]
            if hit["kind"].string == "close" { store?.dispatch(row["close"]["action"]) }
            else if !row["target"].isNull { send(JSON(["kind": "menu_select", "route": row["target"].raw])) }
            else { store?.dispatch(row["action"]) }
        }
    }
    private func send(_ action: JSON) { store?.dispatch(["type": "phone", "action": action.raw]) }
    func key(_ key: String, latin: String?, command: Bool, shift: Bool, alt: Bool) {
        guard enabled else { return }
        for section in page["sections"].array {
                if let row = section.array.first(where: { row in
                    row["enabled"].bool && !row["action"].isNull && row["bindings"].array.contains { binding in
                        menuBindingMatches(binding, key: key, latin: latin, command: command, shift: shift, alt: alt)
                    }
                }) { cancel(); store?.dispatch(row["action"]); return }
        }
        guard !command, !alt else { return }
        if key == "escape" || key == (rtl ? "arrowright" : "arrowleft") { send(JSON(["kind": "menu_back", "route": route.raw])); return }
        let rows = order
        guard !rows.isEmpty else { return }
        let current = focused.flatMap { rows.firstIndex(of: $0) }
        switch key {
        case "home": focused = rows.first
        case "end": focused = rows.last
        case "arrowdown", "tab": focused = rows[((current ?? (shift ? 0 : -1)) + (shift ? rows.count - 1 : 1)) % rows.count]
        case "arrowup": focused = rows[((current ?? 0) + rows.count - 1) % rows.count]
        case "enter", " ": if let focused, let hit = try? JSON.decode(focused) { activate(hit, semantic: true) }
        default:
            if key == (rtl ? "arrowleft" : "arrowright"), let focused, let hit = try? JSON.decode(focused), hit["kind"].string == "row" {
                let row = page["sections"][Int(hit["position"][0].number)][Int(hit["position"][1].number)]
                if !row["target"].isNull { activate(hit, semantic: true) }
            }
        }
    }
}

struct PhoneMenu: View {
    @ObservedObject var store: EditorStore
    @StateObject private var input: PhoneMenuPresentation
    @Environment(\.layoutDirection) private var direction
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var key: String { JSON([store.snapshot["layout"]["phone"].raw, store.state["phone"].raw,
        store.state["revision"].raw, store.snapshot["document_tabs"].raw, store.interfaceLanguage]).stableKey }
    init(store: EditorStore, input: PhoneMenuPresentation? = nil) {
        self.store = store; _input = StateObject(wrappedValue: input ?? PhoneMenuPresentation(store: store))
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            if !input.root.isNull {
                page(input.root, live: false).fixedSize(horizontal: true, vertical: true)
                    .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { input.measured(natural: $0) }
                    .hidden().accessibilityHidden(true)
                if input.listWidth > 0 {
                    page(input.root, live: false).frame(width: input.listWidth).fixedSize(horizontal: false, vertical: true)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { input.measured(height: $0) }
                        .hidden().accessibilityHidden(true)
                }
                if !input.frame["placement"].isNull { surface }
            }
        }.font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background { if input.open || !input.frame.isNull { NativeReorderInput(model: input) } }
            .background { if input.enabled { ShortcutKeyCapture(captured: input.key).frame(width: 1, height: 1) } }
            .onChange(of: key, initial: true) { _, _ in
                input.rtl = direction == .rightToLeft; input.reduceMotion = reduceMotion
                input.refresh(layout: store.snapshot["layout"]["phone"], phone: store.state["phone"])
            }.onDisappear { input.disappear() }
    }
    private var surface: some View {
        let placement = input.frame["placement"], bounds = placement["bounds"], connection = input.frame["connection"]
        return ZStack(alignment: .topLeading) {
            if !connection.isNull {
                DrawerBridge(connection: connection).fill(input.drawer.glassShown ? palette.glassPanel : palette["panel"]).placed(connection["bounds"])
                    .allowsHitTesting(false)
                    .accessibilityIdentifier("phone-menu-bridge")
            }
            ZStack(alignment: .topLeading) {
                let list = input.geometry["list"]
                ZStack(alignment: .topLeading) {
                    if input.swiping, !input.parent.isNull {
                        page(input.parent, live: false).frame(width: list["width"].number)
                            .mask(alignment: input.rtl ? .trailing : .leading) { Rectangle().frame(width: list["width"].number * input.progress) }
                    }
                    ScrollViewReader { reader in
                        EditorPositionedScrollView(position: input.position, offsetChanged: { _ in input.cancelPress() }) {
                            page(input.page, live: true).id(input.route.stableKey)
                        }.modifier(EditorScrollActivity { input.cancelPress() })
                            .onChange(of: input.focused) { _, hit in if let hit { reader.scrollTo(hit) } }
                    }.offset(x: list["width"].number * input.progress * (input.rtl ? -1 : 1))
                }.clipped().placed(list).accessibilityIdentifier("phone-menu-scroll")
                Rectangle().fill(palette.panelBorder).placed(input.geometry["separator"])
                ForEach(input.model["strip"].array.indices, id: \.self) { index in
                    stripButton(index).placed(input.geometry["buttons"][index])
                }
            }.frame(width: input.target["bounds"]["width"].number, height: input.target["bounds"]["height"].number, alignment: .topLeading)
                .frame(width: bounds["width"].number, height: bounds["height"].number, alignment: .topLeading).clipped()
                .modifier(DrawerContainerSurface(drawers: store.contentDrawers, bounds: bounds.rect,
                    cuts: DrawerShadow.cuts(placement: placement, connection: connection), fill: input.drawer.glassShown ? palette.glassPanel : palette["panel"],
                    shadow: 6, shadowOpacity: 0.16, joined: connection["square_corners"], excluding: input.drawer.id, drawer: input.drawer,
                    identifier: "phone-menu"))
                .placed(bounds).accessibilityLabel(input.page["title"].string).allowsHitTesting(input.enabled)
        }
    }
    private func stripButton(_ index: Int) -> some View {
        let item = input.model["strip"][index], hit = PhoneMenuPresentation.hit("strip", index)
        let identifier = "phone-" + ["preferences", "search", "finger", "zen", "swap"][index]
        return Button { input.activate(hit) } label: {
            SharedIcon(name: item["icon"].string, size: input.geometry["icon"].number)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }.buttonStyle(HeaderButtonStyle(selected: item["selected"].bool, inBar: true, radius: input.rowHeight / 2))
            .disabled(!item["enabled"].bool).help(item["label"].string).accessibilityLabel(item["label"].string)
            .accessibilityAddTraits(item["selected"].bool ? .isSelected : [])
            .accessibilityIdentifier(identifier)
            .accessibilityAction { input.activate(hit, semantic: true) }
            .modifier(PhoneMenuHit(input: input, hit: hit))
    }
    private func page(_ page: JSON, live: Bool) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            if !page["parent"].isNull {
                let hit = PhoneMenuPresentation.hit("back")
                Button { input.activate(hit) } label: {
                    HStack(spacing: 8) {
                        SharedIcon(name: "back", size: 24).scaleEffect(x: input.rtl ? -1 : 1)
                        Text(page["title"].string).lineLimit(1).frame(maxWidth: .infinity).multilineTextAlignment(.center)
                        Color.clear.frame(width: 24)
                    }.padding(.horizontal, 16).frame(minHeight: 36)
                }.buttonStyle(.plain).accessibilityIdentifier("phone-menu-back")
                    .accessibilityAction { input.activate(hit, semantic: true) }
                    .modifier(PhoneMenuHit(input: live ? input : nil, hit: hit))
            }
            ForEach(page["sections"].array.indices, id: \.self) { section in
                if section > 0 { EditorLine(color: palette.panelBorder).padding(.horizontal, 6).padding(.vertical, 4) }
                ForEach(page["sections"][section].array.indices, id: \.self) { index in
                    row(page["sections"][section][index], page: page, section: section, index: index, live: live)
                }
            }
        }.padding(.vertical, 6).allowsHitTesting(live).accessibilityHidden(!live)
    }
    @ViewBuilder private func row(_ row: JSON, page: JSON, section: Int, index: Int, live: Bool) -> some View {
        if row["is_heading"].bool {
            Text(row["label"].string).foregroundStyle(.secondary).padding(.horizontal, 10).frame(minHeight: 36)
                .accessibilityAddTraits(.isHeader)
        } else { actionRow(row, page: page, section: section, index: index, live: live) }
    }
    private func actionRow(_ row: JSON, page: JSON, section: Int, index: Int, live: Bool) -> some View {
        let hit = PhoneMenuPresentation.hit("row", [section, index])
        return HStack(spacing: 0) {
            Button { input.activate(hit) } label: {
                EditorMenuRowLabel(label: row["label"].string, icon: row["icon"].string,
                    selected: row["selected"].isNull ? nil : row["selected"].bool, checks: page["checks"].bool,
                    hint: row["count"].string.isEmpty ? row["hint"].string : row["count"].string,
                    submenu: !row["target"].isNull, singleLine: true, middleEllipsis: row["ellipsis"].string == "middle")
            }.buttonStyle(.plain).disabled(!row["enabled"].bool)
                .background(input.pressed == hit.stableKey || input.focused == hit.stableKey ? palette.accent.opacity(0.15) : .clear,
                    in: RoundedRectangle(cornerRadius: 4))
                .accessibilityAddTraits(row["selected"].bool ? .isSelected : [])
                .accessibilityIdentifier(row["target"]["page"]["kind"].string == "drawings" ? "phone-drawing" : "phone-menu-row-\(section)-\(index)")
                .accessibilityAction { input.activate(hit, semantic: true) }
                .modifier(PhoneMenuHit(input: live ? input : nil, hit: hit))
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height in
                    if page["parent"].isNull && section == 0 && index == 0 { input.measured(row: height) }
                }
            if !row["close"].isNull {
                let close = PhoneMenuPresentation.hit("close", [section, index])
                Button { input.activate(close) } label: { SharedIcon(name: "close", size: 16).frame(maxWidth: .infinity, maxHeight: .infinity) }
                    .buttonStyle(.plain).frame(width: input.model["close_width"].number, height: input.rowHeight)
                    .disabled(!row["enabled"].bool)
                    .accessibilityLabel(row["close"]["label"].string).accessibilityIdentifier("phone-menu-close-\(section)-\(index)")
                    .accessibilityAction { input.activate(close, semantic: true) }
                    .modifier(PhoneMenuHit(input: live ? input : nil, hit: close))
            }
        }.accessibilityElement(children: .contain)
            .accessibilityIdentifier(row["target"].isNull ? "phone-menu-item" : "phone-menu-submenu")
    }
}

private struct PhoneMenuHit: ViewModifier {
    let input: PhoneMenuPresentation?
    let hit: JSON
    func body(content: Content) -> some View {
        content.id(hit.stableKey)
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("editor-workspace")) } action: { input?.allocated(hit, $0) }
    }
}
