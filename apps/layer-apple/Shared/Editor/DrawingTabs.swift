import SwiftUI
import UniformTypeIdentifiers

@MainActor final class DrawingTabsController: ObservableObject {
    @Published private(set) var view = JSON()
    @Published private(set) var busy = false
    @Published var presented = false
    private(set) var confirmingWindow = false
    private weak var store: EditorStore?
    private var request: UInt64?
    init(store: EditorStore) { self.store = store }
    var rows: [JSON] { view["tabs"].array }
    var selected: UInt64 { view["selected"].uint }
    var selectedDescription: String {
        guard let tab = store?.state["tabs"][0] else { return "" }
        return "\(tab["title"].string) · \(tab["width"].uint) × \(tab["height"].uint)"
    }
    func receive() {
        guard let store else { return }
        let next = store.snapshot["document_tabs"]
        if !SnapshotProjection.equal(view.raw, next.raw) { view = next }
        if let command = store.state["requests"].array.first(where: { $0["kind"]["type"].string == "drawings" }), request != command["id"].uint {
            request = command["id"].uint; presented = true
            store.dispatch(["type": "complete_request", "id": command["id"].uint, "error": NSNull()])
        }
    }
    func select(_ id: UInt64, completion: @escaping (Bool) -> Void = { _ in }) {
        guard let store, !busy, !store.projectFiles.busy else { completion(false); return }
        if id == selected { completion(true); return }
        busy = true
        store.recovery.flush { [weak self] saved in
            guard let self else { completion(false); return }
            guard saved else { busy = false; store.projectFiles.error = "Could not preserve the current drawing for recovery"; completion(false); return }
            store.native?.switchDocument(id) { [weak self] error in DispatchQueue.main.async {
                guard let self else { completion(false); return }
                self.busy = false; if let error { store.projectFiles.error = error }
                store.wake?(); completion(error == nil)
            } }
        }
    }
    func adjacent(_ forward: Bool) {
        store?.query(["type": "document_tabs", "op": "adjacent", "forward": forward]) { [weak self] id in
            if !id.isNull { self?.select(id.uint) }
        }
    }
    func edit(_ value: [String: Any]) {
        guard !busy else { return }
        store?.query(["type": "document_tabs"].merging(value) { _, v in v }) { _ in }
    }
    func drop(id: UInt64, hits: [[String: Any]], point: CGPoint, vertical: Bool, completion: @escaping (JSON) -> Void) {
        store?.query(["type": "document_tabs", "op": "drop", "hits": hits, "point": [point.x, point.y], "vertical": vertical], completion: completion)
    }
    func close(_ id: UInt64) {
        presented = false
        select(id) { [weak self] selected in
            guard selected, let self, let store else { return }
            store.projectFiles.confirmCurrentClose { [weak self] allowed in if allowed { self?.closeApproved() } }
        }
    }
    func closeApproved() {
        guard let store, !busy, !confirmingWindow else { return }
        if rows.count <= 1 { store.projectFiles.closeWindow?(); return }
        busy = true
        let id = selected, recovery = store.recovery
        recovery.close { [weak self] saved in
            guard let self else { return }
            guard saved else { busy = false; store.projectFiles.error = "Could not retire this drawing's recovery copy"; return }
            store.native?.switchDocument(0, closing: true) { [weak self] error in DispatchQueue.main.async {
                guard let self else { return }
                self.busy = false
                if let error { store.projectFiles.error = error; recovery.resume() }
                else { store.forgetRecovery(id) }
                store.wake?()
            } }
        }
    }
    func confirmWindowClose(_ completion: @escaping (Bool) -> Void) {
        guard !busy, !confirmingWindow, let store else { completion(false); return }
        confirmingWindow = true
        var remaining = [selected] + rows.map { $0["id"].uint }.filter { $0 != selected }
        func next() {
            guard !remaining.isEmpty else { confirmingWindow = false; completion(true); return }
            let id = remaining.removeFirst()
            select(id) { selected in
                guard selected else { finish(false); return }
                store.projectFiles.confirmCurrentClose { allowed in if allowed { next() } else { finish(false) } }
            }
        }
        func finish(_ allowed: Bool) {
            confirmingWindow = false
            if !allowed { edit(["op": "reset_close"]) }
            completion(allowed)
        }
        next()
    }
}

struct DrawingFrame: Equatable {
    var body = CGRect.zero
    var grip = CGRect.zero
    var close = CGRect.zero
}
private struct DrawingFrames: PreferenceKey {
    static var defaultValue: [UInt64: DrawingFrame] { [:] }
    static func reduce(value: inout [UInt64: DrawingFrame], nextValue: () -> [UInt64: DrawingFrame]) {
        value.merge(nextValue()) { a, b in DrawingFrame(body: b.body == .zero ? a.body : b.body,
            grip: b.grip == .zero ? a.grip : b.grip, close: b.close == .zero ? a.close : b.close) }
    }
}
private struct DrawingMeasure: ViewModifier {
    let id: UInt64
    var part: WritableKeyPath<DrawingFrame, CGRect> = \.body
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
            let rect = geometry.frame(in: .named("drawing-tabs"))
            let frame = { var frame = DrawingFrame(); frame[keyPath: part] = rect; return frame }()
            Color.clear.preference(key: DrawingFrames.self, value: [id: frame])
        })
    }
}
@MainActor final class DrawingTabInteraction: ObservableObject, NativeReorderModel {
    let contact = ReorderContact()
    var viewport = CGRect.zero
    var enabled = true
    var vertical = false
    var frames: [UInt64: DrawingFrame] = [:]
    weak var controller: DrawingTabsController?
    @Published var dragged: UInt64?
    @Published var before: JSON?
    @Published var menu: UInt64?
    private var motion: UInt64 = 0
    private var hits: [[String: Any]] {
        frames.map { id, f in ["id": id, "bounds": ["x": f.body.minX, "y": f.body.minY, "width": f.body.width, "height": f.body.height]] }
    }
    func cancel() { contact.cancel(); dragged = nil; before = nil; menu = nil; motion &+= 1 }
    func acceptsContext(at point: CGPoint) -> Bool { enabled && viewport.contains(point) && frames.values.contains { $0.body.contains(point) } }
    func context(at point: CGPoint) { if acceptsContext(at: point) { menu = frames.first { $0.value.body.contains(point) }?.key } }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard enabled, viewport.contains(point), let (id, frame) = frames.first(where: { $0.value.body.contains(point) }), !frame.close.contains(point) else { return nil }
        menu = nil
        return ReorderTarget(id: String(id), surface: !vertical || frame.grip.contains(point) ? .handle : .row,
            valid: { [weak self] _ in self?.enabled == true && self?.controller?.rows.contains { $0["id"].uint == id } == true },
            openContext: { [weak self] in self?.menu = id }, closeContext: { [weak self] in self?.menu = nil },
            begin: { [weak self] _ in self?.dragged = id }, move: { [weak self] p in self?.move(id, p) },
            finish: { [weak self] p in
                guard let self else { return }
                motion &+= 1; dragged = nil; before = nil
                guard viewport.contains(p) else { return }
                controller?.drop(id: id, hits: hits, point: p, vertical: vertical) { [weak self] target in
                    if !target.isNull { self?.controller?.edit(["op": "reorder", "id": id, "before": target["before"].raw]) }
                }
            }, cancel: { [weak self] in self?.dragged = nil; self?.before = nil; self?.motion &+= 1 })
    }
    private func move(_ id: UInt64, _ point: CGPoint) {
        motion &+= 1; let token = motion
        guard viewport.contains(point) else { before = nil; return }
        controller?.drop(id: id, hits: hits, point: point, vertical: vertical) { [weak self] target in
            guard let self, motion == token, dragged == id else { return }
            before = target.isNull ? nil : target["before"]
        }
    }
    var marker: CGRect? {
        guard let before else { return nil }
        let bounds: CGRect?
        if before.isNull { bounds = frames.values.map(\.body).max { vertical ? $0.maxY < $1.maxY : $0.maxX < $1.maxX } }
        else { bounds = frames[before.uint]?.body }
        guard let b = bounds else { return nil }
        return vertical ? CGRect(x: b.minX, y: (before.isNull ? b.maxY : b.minY) - 1, width: b.width, height: 2)
            : CGRect(x: (before.isNull ? b.maxX : b.minX) - 1, y: b.minY, width: 2, height: b.height)
    }
}

struct DrawingTabsHeader: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var tabs: DrawingTabsController
    let width: CGFloat
    var body: some View {
        Group {
            if tabs.rows.count > 1 && width / CGFloat(tabs.rows.count) >= 140 {
                DrawingTabList(store: store, tabs: tabs, vertical: false)
            } else {
                Button { tabs.presented = true } label: {
                    HStack(spacing: 5) {
                        Text(tabs.rows.first { $0["id"].uint == tabs.selected }?["title"].string ?? "Untitled").lineLimit(1)
                        if tabs.rows.count > 1 { Image(systemName: "chevron.down").font(.caption) }
                    }.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityIdentifier("document-title")
                    .accessibilityValue(tabs.selectedDescription).help(tabs.selectedDescription)
            }
        }.modifier(DrawingOpenDrop(store: store))
    }
}

private struct DrawingTabList: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var tabs: DrawingTabsController
    let vertical: Bool
    @StateObject private var interaction = DrawingTabInteraction()
    var body: some View {
        let layout = vertical ? AnyLayout(VStackLayout(spacing: 2)) : AnyLayout(HStackLayout(spacing: 2))
        layout {
            ForEach(tabs.rows, id: \.drawingID) { row in
                let id = row["id"].uint
                HStack(spacing: 5) {
                    if vertical {
                        SharedIcon(name: "grip", size: 12).frame(width: 24, height: 44).contentShape(Rectangle())
                            .modifier(DrawingMeasure(id: id, part: \.grip)).accessibilityLabel("Move drawing")
                    }
                    Button {
                        guard !interaction.contact.consumeClick() else { return }
                        tabs.select(id) { if $0 && vertical { tabs.presented = false } }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row["title"].string + (row["modified"].bool ? " •" : "")).lineLimit(1)
                            if vertical { Text(row["location"].string).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading).contentShape(Rectangle())
                    }.buttonStyle(.plain).help(row["location"].string)
                        .accessibilityIdentifier("drawing-tab-\(id)").accessibilityAddTraits(id == tabs.selected ? .isSelected : [])
                        .accessibilityValue(id == tabs.selected ? tabs.selectedDescription : row["location"].string)
                    Button { tabs.close(id) } label: { Image(systemName: "xmark").font(.caption).frame(width: 24, height: vertical ? 44 : 28).contentShape(Rectangle()) }
                        .buttonStyle(.plain).accessibilityLabel("Close " + row["title"].string).accessibilityIdentifier("drawing-close-\(id)")
                        .modifier(DrawingMeasure(id: id, part: \.close))
                }.padding(.horizontal, 7).frame(height: vertical ? 48 : 30)
                    .background(id == tabs.selected ? Color.primary.opacity(0.12) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 6))
                    .opacity(interaction.dragged == id ? 0.4 : 1)
                    .modifier(DrawingMeasure(id: id))
                    .editorPopover(isPresented: Binding(get: { interaction.menu == id }, set: { if !$0 { interaction.menu = nil } })) {
                        VStack(alignment: .leading) {
                            Button("Move Earlier") { tabs.edit(["op":"step", "id":id,"forward":false]); interaction.menu = nil }
                            Button("Move Later") { tabs.edit(["op":"step", "id":id,"forward":true]); interaction.menu = nil }
                            Button("Close Drawing") { interaction.menu = nil; tabs.close(id) }
                        }.padding(12)
                    }
            }
        }.coordinateSpace(name: "drawing-tabs")
            .background(NativeReorderInput(model: interaction))
            .onPreferenceChange(DrawingFrames.self) { interaction.frames = $0 }
            .onAppear { update() }.onChange(of: tabs.view.stableKey) { _, _ in update() }
            .onChange(of: tabs.busy) { _, _ in update() }.onDisappear { interaction.cancel() }
            .overlay(alignment: .topLeading) {
                if let marker = interaction.marker { Rectangle().fill(Color.accentColor).frame(width: marker.width, height: marker.height).offset(x: marker.minX, y: marker.minY).allowsHitTesting(false) }
            }
            .onScrollPhaseChange { _, phase in if phase != .idle && !interaction.contact.held && !interaction.contact.dragging { interaction.cancel() } }
    }
    private func update() {
        interaction.controller = tabs; interaction.vertical = vertical
        interaction.enabled = !tabs.busy && !store.projectFiles.busy
        if interaction.contact.target != nil { _ = interaction.contact.validate() }
    }
}
private extension JSON { var drawingID: UInt64 { self["id"].uint } }

struct DrawingTabsPresentation: ViewModifier {
    @ObservedObject var store: EditorStore
    @ObservedObject var tabs: DrawingTabsController
    func body(content: Content) -> some View {
        content.allowsHitTesting(!tabs.busy).overlay {
            if tabs.busy { ProgressView("Switching drawing…").padding(20).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12)) }
        }.sheet(isPresented: $tabs.presented) {
            VStack(alignment: .leading, spacing: 12) {
                Text("Drawings").font(.headline)
                ScrollView { DrawingTabList(store: store, tabs: tabs, vertical: true) }
                if !tabs.view["storage_error"].isNull { Text(tabs.view["storage_error"].string).foregroundStyle(.red) }
                HStack {
                    Button("Undo Tab Order") { tabs.edit(["op":"history","redo":false]) }.disabled(!tabs.view["can_undo"].bool)
                    Button("Redo Tab Order") { tabs.edit(["op":"history","redo":true]) }.disabled(!tabs.view["can_redo"].bool)
                    Spacer(); Button("Done") { tabs.presented = false }.keyboardShortcut(.cancelAction)
                }
            }.padding(20).frame(minWidth: 340, idealWidth: 480, minHeight: 260, idealHeight: 440)
                .modifier(EditorPopupPresentation()).modifier(EditorPopoverHost())
                .accessibilityElement(children: .contain).accessibilityIdentifier("drawing-selector")
        }
    }
}

private struct DrawingOpenDrop: ViewModifier {
    @ObservedObject var store: EditorStore
    func body(content: Content) -> some View {
        content.onDrop(of: [UTType.fileURL, .capyProject] + UTType.capyPhotoTypes, isTargeted: nil) { providers in
            guard !store.projectFiles.busy, !store.drawingTabs.busy else { return false }
            let items = providers.compactMap(PhotoItem.drawingProvider)
            guard !items.isEmpty else { return false }
            store.projectFiles.openItems(items); return true
        }
    }
}
