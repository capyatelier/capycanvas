import SwiftUI
import UniformTypeIdentifiers

struct PaletteMessage: Equatable {
    let text: String
    let error: Bool
}

enum PaletteMenuTarget: Equatable {
    case color(UInt64), palette(UInt64), library
    var request: [String: Any] {
        switch self {
        case .color(let id): ["kind": "color", "id": id]
        case .palette(let id): ["kind": "palette", "id": id]
        case .library: ["kind": "library"]
        }
    }
}

struct PaletteMenu {
    let target: PaletteMenuTarget
    let owner: UUID
    let model: JSON
}

struct PaletteDialog: Identifiable {
    enum Kind { case new, rename, remove }
    let id = UUID()
    let kind: Kind
    var palette: UInt64?
    var name = ""
}

struct PaletteExport {
    let document: PaletteFileDocument
    let name: String
    let type: UTType
    let notice: String?
}

struct PaletteLift: Equatable {
    let owner: UUID
    let id: UInt64
    var frame: CGRect
    let rgba: JSON
    let color: JSON
    let selected: Bool
    static func == (a: Self, b: Self) -> Bool { a.owner == b.owner && a.id == b.id && a.frame == b.frame && a.selected == b.selected }
}

struct PaletteFileDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.data] }
    let data: Data
    init(data: Data) { self.data = data }
    init(configuration: ReadConfiguration) throws { data = configuration.file.regularFileContents ?? Data() }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: data) }
}

@MainActor final class PaletteController: ObservableObject {
    private weak var store: EditorStore?
    @Published var selected: UInt64?
    @Published var chooser = false
    @Published var expanded = false
    @Published private(set) var editing = false
    @Published var editText = ""
    @Published var message: PaletteMessage?
    @Published var dialog: PaletteDialog?
    @Published private(set) var menu: PaletteMenu?
    @Published var importing = false
    @Published var export: PaletteExport?
    @Published var lift: PaletteLift?
    @Published var settle: [UInt64]?
    var focused = false
    private var editColor = ""
    private var menuRequest = UUID()
    private var swallowEscapeRelease = false

    init(store: EditorStore) { self.store = store }

    var view: JSON { store?.snapshot["palette_panel"] ?? JSON() }
    var current: JSON { store?.snapshot["color_panel"]["definition"] ?? JSON() }
    func selection(_ view: JSON) -> JSON? {
        let swatches = view["swatches"].array
        return swatches.first { $0["id"].uint == selected && $0["current"].bool } ?? swatches.first { $0["current"].bool }
    }

    func apply(_ action: Any, done: @escaping (String?) -> Void = { _ in }) {
        guard let store else { return }
        store.edit(["type": "color", "action": ["op": "library", "action": action]]) { [weak self] error in
            self?.message = error.map { PaletteMessage(text: $0, error: true) }
            done(error)
        }
    }
    func dryRun(_ action: [String: Any], done: @escaping (String?) -> Void) {
        store?.query(["type": "palette_action", "action": action, "dry_run": true]) { result in
            done(result["error"].isNull ? nil : result["error"].string)
        }
    }

    func addCurrent() {
        let view = self.view, color = current
        guard !view.isNull, !color.isNull else { return }
        let save: () -> Void = { [weak self] in
            self?.apply(["op": "store", "palette": view["palette"].raw, "name": "", "color": color.raw]) { [weak self] error in
                guard let self, error == nil else { return }
                self.selected = self.view["swatches"].array.last?["id"].uint
            }
        }
        if editing { commitName(editText, then: save) } else { save() }
    }
    func use(_ id: UInt64) {
        focused = true; selected = id
        apply(["op": "use", "id": id])
    }
    func useRecent(_ color: JSON) {
        focused = true; selected = nil
        store?.dispatch(["type": "color", "action": ["op": "definition", "color": color.raw]])
    }
    func choose(_ id: UInt64) {
        apply(["op": "select_palette", "id": id]) { [weak self] error in if error == nil { self?.chooser = false } }
    }
    func toggleChooser() {
        if editing { commitName(editText) }
        expanded = false; chooser.toggle()
    }

    func beginEditing() {
        chooser = false; expanded = false
        editColor = current.stableKey
        editText = selection(view)?["name"].string ?? view["color_name"].string
        editing = true
    }
    func cancelEditing() { editing = false; message = nil }
    func retireStaleEdit() {
        if editing && current.stableKey != editColor { editing = false; message = nil }
    }
    func commitName(_ text: String, then done: @escaping () -> Void = {}) {
        guard editing else { return }
        let view = self.view
        let action: [String: Any]
        if let id = selection(view)?["id"].uint { action = ["op": "rename", "id": id, "name": text] }
        else if !current.isNull { action = ["op": "name_current", "name": text, "color": current.raw] }
        else { return }
        editing = false
        apply(action) { [weak self] error in
            guard let self else { return }
            if error != nil { editColor = current.stableKey; editText = text; editing = true } else { done() }
        }
    }

    func openMenu(_ target: PaletteMenuTarget, owner: UUID) {
        guard lift == nil, let store else { return }
        let request = UUID(); menuRequest = request
        store.query(["type": "palette_menu", "target": target.request]) { [weak self] sections in
            guard let self, menuRequest == request, lift == nil, !sections.array.isEmpty else { return }
            menu = PaletteMenu(target: target, owner: owner, model: JSON(["title": title(target), "sections": sections.raw]))
        }
    }
    private func title(_ target: PaletteMenuTarget) -> String {
        switch target {
        case .color(let id): view["swatches"].array.first { $0["id"].uint == id }?["name"].string ?? ""
        case .palette(let id): view["palettes"].array.first { $0["id"].uint == id }?["name"].string ?? ""
        case .library: "Palettes"
        }
    }
    func closeMenu(owner: UUID? = nil) {
        guard owner == nil || menu?.owner == owner else { return }
        menuRequest = UUID(); if menu != nil { menu = nil }
    }
    func menuPresented(_ target: PaletteMenuTarget, owner: UUID) -> Binding<Bool> {
        Binding(get: { [weak self] in self?.menu?.target == target && self?.menu?.owner == owner },
            set: { [weak self] presented in
                if !presented, self?.menu?.target == target, self?.menu?.owner == owner { self?.closeMenu() }
            })
    }
    func command(_ command: JSON) {
        closeMenu()
        let view = self.view
        switch command["command"].string {
        case "new_palette": dialog = PaletteDialog(kind: .new)
        case "import_palette": importing = true
        case "rename_palette", "remove_palette":
            guard let palette = view["palettes"].array.first(where: { $0["id"].uint == command["id"].uint }) else { return }
            dialog = PaletteDialog(kind: command["command"].string == "rename_palette" ? .rename : .remove,
                palette: palette["id"].uint, name: palette["name"].string)
        case "export_palette": exportPalette(id: command["id"].uint, format: command["format"].string)
        case "rename_color":
            let id = command["id"].uint
            guard view["swatches"].array.contains(where: { $0["id"].uint == id }) else { return }
            selected = id
            apply(["op": "use", "id": id]) { [weak self] error in if error == nil { self?.beginEditing() } }
        case "library": apply(command["action"].raw)
        default: break
        }
    }

    func importPalette(_ result: Result<URL, Error>) {
        switch result {
        case .success(let url):
            NativeProjectTask.io.async {
                let action = Result { try ColorPreferencesStore.importPalette(url) }
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    switch action {
                    case .success(let action):
                        apply(action.raw) { [weak self] error in if error == nil { self?.chooser = false } }
                    case .failure(let error): message = PaletteMessage(text: error.localizedDescription, error: true)
                    }
                }
            }
        case .failure(let error): report(error)
        }
    }
    private func exportPalette(id: UInt64, format: String) {
        guard let palette = store?.state["colors"]["library"]["palettes"].array.first(where: { $0["id"].uint == id }) else { return }
        NativeProjectTask.io.async {
            let encoded = Result { try ColorPreferencesStore.exportPalette(palette, format: format) }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                switch encoded {
                case .success(let (metadata, data)):
                    let name = metadata["file_name"].string
                    export = PaletteExport(document: PaletteFileDocument(data: data), name: name,
                        type: UTType(filenameExtension: (name as NSString).pathExtension) ?? .data,
                        notice: metadata["notice"].isNull ? nil : metadata["notice"].string)
                case .failure(let error): message = PaletteMessage(text: error.localizedDescription, error: true)
                }
            }
        }
    }
    func exported(_ result: Result<URL, Error>) {
        let notice = export?.notice
        export = nil
        switch result {
        case .success: message = notice.map { PaletteMessage(text: $0, error: false) }
        case .failure(let error): report(error)
        }
    }
    private func report(_ error: Error) {
        let e = error as NSError
        if e.domain != NSCocoaErrorDomain || e.code != NSUserCancelledError {
            message = PaletteMessage(text: error.localizedDescription, error: true)
        }
    }

    func undoReorder(redo: Bool) -> Bool {
        let view = self.view
        guard focused, !view.isNull else { return false }
        if view[redo ? "can_redo" : "can_undo"].bool {
            apply(["op": redo ? "redo_reorder" : "undo_reorder", "palette": view["palette"].raw])
        }
        return true
    }
    func escape() -> Bool {
        if lift != nil { lift = nil; return true }
        if menu != nil { closeMenu(); return true }
        if editing { cancelEditing(); return true }
        if chooser { chooser = false; return true }
        if expanded { expanded = false; return true }
        return false
    }
    func key(_ key: String, pressed: Bool, command: Bool, shift: Bool) -> Bool {
        let name = key.lowercased()
        if name == "escape" {
            if pressed { swallowEscapeRelease = escape(); return swallowEscapeRelease }
            defer { swallowEscapeRelease = false }
            return swallowEscapeRelease
        }
        guard command, name == "z" || name == "y", focused else { return false }
        return !pressed || undoReorder(redo: name == "y" || shift)
    }
}
