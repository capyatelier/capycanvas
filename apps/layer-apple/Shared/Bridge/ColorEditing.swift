import SwiftUI

@MainActor final class ColorEditingController: ObservableObject {
    private weak var store: EditorStore?
    @Published private(set) var session: ColorEditorSession?
    init(store: EditorStore) { self.store = store }
    func open(colors: JSON, slot: String? = nil, value: JSON? = nil, opaque: Bool = false, viewing: JSON = JSON(),
        use: @escaping (JSON, Double?) -> Void) {
        guard let store else { return }
        session?.close(apply: false)
        session = ColorEditorSession(controller: self, store: store, colors: colors, slot: slot, value: value,
            opaque: opaque, viewing: viewing, use: use)
    }
    func ended(_ session: ColorEditorSession) { if self.session === session { self.session = nil } }
    func receive(_ picker: JSON) { session?.receive(picker) }
}

@MainActor final class ColorEditorSession: ObservableObject, Identifiable {
    private weak var controller: ColorEditingController?
    private weak var store: EditorStore?
    private let epoch: UInt64
    private let memory: String
    private let use: (JSON, Double?) -> Void
    private var refused: [String: Any]?
    private var pickStarted = false
    let viewing: JSON
    let rendition: JSON
    @Published private(set) var editor = JSON()
    @Published private(set) var view = JSON()
    @Published private(set) var error: String?
    @Published private(set) var errorTarget: String?
    @Published private(set) var picking = false
    @Published private(set) var recent: [JSON] = []
    @Published private(set) var sheet = JSON()
    var blocked: Bool { refused != nil || view["value"].isNull }
    var search: String { editor["picker"]["editor"]["search"].string }
    private var language: String { store?.interfaceLanguage ?? "en" }

    init(controller: ColorEditingController?, store: EditorStore?, colors: JSON, slot: String? = nil, value: JSON? = nil, opaque: Bool = false,
        viewing: JSON = JSON(), use: @escaping (JSON, Double?) -> Void) {
        self.controller = controller; self.store = store; self.viewing = viewing; self.use = use
        epoch = store?.state["document_file"]["epoch"].uint ?? 0
        rendition = store?.snapshot["color_panel"]["rendition"] ?? JSON()
        var request: [String: Any] = ["type": "editor_open", "opaque": opaque, "display_space": "DisplayP3", "rendition": rendition.raw]
        if !colors.isNull { request["colors"] = colors.raw }
        if let slot { request["slot"] = slot } else if let value { request["color"] = value.raw }
        let opened = ColorUI.resolve(request, language: store?.interfaceLanguage ?? "en")
        memory = opened["editor"]["picker"]["editor"].stableKey
        editor = opened["editor"]; view = opened["view"]
        if editor.isNull { error = opened["error"].string; refused = [:] }
        refreshSwatches(sheetOpen: false)
    }
    private func send(_ action: [String: Any]?) -> String? {
        var request: [String: Any] = ["type": "editor", "editor": editor.raw, "display_space": "DisplayP3", "rendition": rendition.raw]
        if let action { request["action"] = action }
        let next = ColorUI.resolve(request, language: language)
        if next["editor"].isNull { return next["error"].string }
        editor = next["editor"]; view = next["view"]
        return next["error"].isNull ? nil : next["error"].string
    }
    @discardableResult func act(_ action: [String: Any], target: String? = nil) -> Bool {
        guard !editor.isNull else { return false }
        let failure = send(action)
        if failure != nil || target == errorTarget || errorTarget == nil {
            error = failure; errorTarget = failure == nil ? nil : target; refused = failure == nil ? nil : action
        }
        return failure == nil
    }
    func dismissError(_ target: String) {
        guard errorTarget == target else { return }
        error = nil; errorTarget = nil; refused = nil
    }
    func relocalize() {
        guard !editor.isNull else { return }
        _ = send(nil)
        if let refused, !refused.isEmpty { error = send(refused) }
    }
    func refreshSwatches(sheetOpen: Bool) {
        store?.query(["type": "swatch_sheet", "query": "", "current": view["value"].raw]) { [weak self] shown in
            self?.recent = shown["sections"].array.first { $0["palette"].isNull }?["tiles"].array ?? []
        }
        guard sheetOpen else { return }
        store?.query(["type": "swatch_sheet", "query": search, "current": view["value"].raw]) { [weak self] shown in self?.sheet = shown }
    }
    func search(_ text: String) {
        _ = send(["op": "search", "text": text])
        refreshSwatches(sheetOpen: true)
    }
    func addToPalette(_ palette: JSON) {
        store?.dispatch(["type": "color", "action": ["op": "library", "action": ["op": "store", "palette": palette.raw, "name": "", "color": view["value"].raw]]])
        refreshSwatches(sheetOpen: true)
    }
    func startPicking(touchOffset: Double) {
        guard !picking, !view["value"].isNull else { return }
        picking = true; pickStarted = false
        store?.dispatch(["type": "color_picker", "action": ["kind": "editor", "original": view["value"].raw, "touch_offset": touchOffset]])
    }
    func stopPicking() { if picking { store?.dispatch(["type": "color_picker", "action": ["kind": "toggle"]]) } }
    func receive(_ picker: JSON) {
        guard picking else { return }
        if picker["editor"].bool { pickStarted = true; return }
        guard pickStarted else { return }
        picking = false
        if !picker["picked"].isNull { act(["op": "color", "color": picker["picked"].raw]) }
    }
    func close(apply: Bool) {
        if picking { stopPicking(); picking = false }
        if !editor.isNull && editor["picker"]["editor"].stableKey != memory {
            store?.dispatch(["type": "color", "action": ["op": "editor_memory", "memory": editor["picker"]["editor"].raw]])
        }
        if apply, !blocked, (store?.state["document_file"]["epoch"].uint ?? 0) == epoch {
            use(view["value"], view["stops"].isNull ? nil : view["stops"].number)
        }
        controller?.ended(self)
    }
}
