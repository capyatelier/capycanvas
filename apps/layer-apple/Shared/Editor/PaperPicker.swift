import SwiftUI
import CoreGraphics

final class NativePaperTask: @unchecked Sendable {
    let handle: UInt64
    init(_ handle: UInt64) { self.handle = handle }
    deinit { let handle = handle; NativeProjectTask.io.async { capy_paper_free(nativeValue(handle)) } }
    func cancel() { capy_paper_cancel(nativeValue(handle)) }
    func work(_ url: URL?) {
        do {
            if let url {
                try ProjectFileIO.coordinate(url, writing: false) { source in
                    let file = try FileHandle(forReadingFrom: source)
                    defer { try? file.close() }
                    source.deletingPathExtension().lastPathComponent.withCString { capy_paper_work(nativeValue(self.handle), file.fileDescriptor, $0) }
                }
            } else { "".withCString { capy_paper_work(nativeValue(handle), -1, $0) } }
        } catch { error.localizedDescription.withCString { capy_paper_failed(nativeValue(handle), $0) } }
    }
}

final class NativePaperPreviews: @unchecked Sendable {
    private let handle: UInt64
    private let ids: [String]
    init(_ handle: UInt64, ids: [String]) { self.handle = handle; self.ids = ids }
    deinit { let handle = handle; NativeProjectTask.io.async { capy_paper_previews_free(nativeValue(handle)) } }
    struct Decoded: @unchecked Sendable { let images: [String: CGImage]; let grains: [String: CGImage] }
    func decode() -> Decoded {
        var images: [String:CGImage] = [:]
        var grains: [String:CGImage] = [:]
        for index in 0..<min(ids.count,capy_paper_previews_count(nativeValue(handle))) {
            var count = 0
            let side = Int(capy_paper_previews_read(nativeValue(handle),index,&count))
            guard side > 0, count == side * side,
                let data = copyNativeData(count, copy: { capy_paper_previews_copy(handle, index, $0, $1, $2) }),
                let provider = CGDataProvider(data:data as CFData),
                let image = CGImage(width:side,height:side,bitsPerComponent:8,bitsPerPixel:8,bytesPerRow:side,
                    space:CGColorSpaceCreateDeviceGray(),bitmapInfo:CGBitmapInfo(rawValue:CGImageAlphaInfo.none.rawValue),
                    provider:provider,decode:nil,shouldInterpolate:true,intent:.relativeColorimetric) else { continue }
            images[ids[index]] = image
            var rgba = [UInt8](repeating: 0, count: count * 4)
            for (pixel, value) in data.enumerated() {
                let shade = Double(value) / 255 - 0.5
                let alpha = UInt8((abs(shade) * (shade < 0 ? 0.7 : 0.25) * 255).rounded())
                let color: UInt8 = shade < 0 ? 0 : alpha
                let offset = pixel * 4
                rgba[offset] = color; rgba[offset + 1] = color; rgba[offset + 2] = color; rgba[offset + 3] = alpha
            }
            if let provider = CGDataProvider(data: Data(rgba) as CFData),
                let grain = CGImage(width: side, height: side, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: side * 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                    provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) {
                grains[ids[index]] = grain
            }
        }
        return Decoded(images: images, grains: grains)
    }
}

@MainActor final class PaperPreviewCache: ObservableObject {
    @Published private(set) var images: [String:CGImage] = [:]
    @Published private(set) var grains: [String:CGImage] = [:]
    private weak var store: EditorStore?
    private var requested = Set<String>()
    init(store: EditorStore) { self.store = store }
    func load(_ id: String) async {
        guard !requested.contains(id), let native = store?.native else { return }
        requested.insert(id)
        let previews: NativePaperPreviews? = await withCheckedContinuation { done in
            native.paperPreviews(ids:[id]) { done.resume(returning:$0) }
        }
        let decoded = await Task.detached(priority:.utility) { previews?.decode() }.value
        if let image = decoded?.images[id], let grain = decoded?.grains[id] {
            grains[id] = grain; images[id] = image
        } else { requested.remove(id) }
    }
}

struct PaperPreview: View {
    @ObservedObject var previews: PaperPreviewCache
    let id: String?
    let side: CGFloat
    var placeholder = "layer-paper-symbolic"
    var body: some View {
        Group {
            if let id, let image = previews.images[id] { Image(decorative:image,scale:1).resizable().frame(width:side,height:side) }
            else { SharedIcon(name:placeholder,size:side / 2) }
        }.frame(width:side,height:side).clipShape(SquircleShape.tile)
            .task(id:id) { if let id { await previews.load(id) } }
    }
}

struct PaperRename: Identifiable {
    let id = UUID()
    let action: JSON
}

struct PaperTexture: Identifiable {
    let id = UUID()
    let layer: UInt64
    let epoch: UInt64
}

@MainActor final class PaperController: ObservableObject {
    private weak var store: EditorStore?
    private let directory: URL?
    private var started = false
    private var handled = Set<UInt64>()
    private var importRequest: JSON?
    private var importEpoch: UInt64 = 0
    private var jobs: [UInt64:NativePaperTask] = [:]
    private var active = Set<UInt64>()
    private var cancelled = Set<UInt64>()
    private var reloadPending = false
    private var afterTexture: JSON?
    let previews: PaperPreviewCache
    @Published var rename: PaperRename?
    @Published var texture: PaperTexture?
    @Published private(set) var generation: UInt64 = 0
    @Published private(set) var busy = false
    init(store: EditorStore, directory: URL?) { self.store = store; self.directory = directory; previews = PaperPreviewCache(store:store) }
    var common: JSON { store?.bootstrap["common"] ?? JSON() }
    var copy: JSON { store?.catalog["native_copy"]["paper"] ?? JSON() }
    func receive() {
        guard let store, directory != nil else { return }
        if !started { started = true; reload() }
        let requests = store.state["requests"].array.filter { $0["kind"]["type"].string == "paper" }
        let current = Set(requests.map { $0["id"].uint })
        handled.formIntersection(current)
        for request in requests where !handled.contains(request["id"].uint) {
            let id = request["id"].uint
            if request["kind"]["request"]["type"].string == "import" {
                guard importRequest == nil else { continue }
                handled.insert(id); importRequest = request; importEpoch = store.state["document_file"]["epoch"].uint
                store.projectFiles.choosePaper { [weak self] url in self?.imported(url) }
            } else { handled.insert(id); run(request) }
        }
    }
    func reload() { run(nil) }
    private func run(_ request: JSON?, url: URL? = nil, epoch: UInt64? = nil) {
        guard let store, let native = store.native, let directory else { return }
        let id = request?["id"].uint ?? 0
        if active.contains(id) { if id == 0 { reloadPending = true }; return }
        active.insert(id); cancelled.remove(id); busy = true
        let complete: @Sendable (String?) -> Void = { [weak self, weak store] error in
            DispatchQueue.main.async {
                guard let self else { return }
                self.jobs.removeValue(forKey:id); self.active.remove(id); self.cancelled.remove(id)
                self.busy = !self.active.isEmpty; self.generation &+= 1
                if let error { store?.failure = error }
                if let request, ["import","add","rename","delete"].contains(request["kind"]["request"]["type"].string), error == nil {
                    store?.paperLibraryChanged()
                }
                if id == 0 && self.reloadPending { self.reloadPending = false; self.reload() }
            }
        }
        native.paperTask(id:UInt32(id),epoch:epoch ?? store.state["document_file"]["epoch"].uint,directory:directory) { [weak self] job,error in
            DispatchQueue.main.async {
                guard let job else { complete(error); return }
                if let self {
                    self.jobs[id] = job
                    if self.cancelled.contains(id) { job.cancel() }
                } else { job.cancel() }
                native.runPaperTask(job,url:url,completion:complete)
            }
        }
    }

    func importToLibrary(_ completion: @escaping @MainActor (String?) -> Void) {
        guard let store, let native = store.native, let directory else { completion(nil); return }
        let finished: @MainActor (String?) -> Void = { [weak self] error in
            if error == nil { self?.store?.paperLibraryChanged() }
            completion(error)
        }
        store.projectFiles.choosePaper { url in
            guard let url else { completion(nil); return }
            native.paperImport(directory: directory) { job, error in
                guard let job else { DispatchQueue.main.async { finished(error) }; return }
                native.runPaperTask(job, url: url) { error in DispatchQueue.main.async { finished(error) } }
            }
        }
    }
    private func imported(_ url: URL?) {
        guard let request = importRequest else { return }
        importRequest = nil
        if let url { run(request,url:url,epoch:importEpoch) }
        else { store?.dispatch(["type":"complete_request","id":request["id"].raw,"error":NSNull()]) }
    }
    func cancelWork() {
        cancelled.formUnion(active)
        for job in jobs.values { job.cancel() }
    }
    func action(_ item: JSON) {
        guard let store else { return }
        if item["action"]["op"].string == "rename_paper" {
            rename = PaperRename(action:item["action"])
        } else { store.dispatch(["type":"effect","action":item["action"].raw]) }
    }
    func choose(layer: UInt64) {
        guard let store else { return }
        texture = PaperTexture(layer:layer,epoch:store.state["document_file"]["epoch"].uint)
    }
    func closeTexture(then item: JSON? = nil) { afterTexture = item; texture = nil }
    func textureClosed() {
        store?.focusCanvas?()
        if let item = afterTexture { afterTexture = nil; action(item) }
    }
    func renamed(_ name: String, completion: @escaping @MainActor (String?) -> Void) {
        guard let rename, let store else { return }
        var action = rename.action.object; action["name"] = name
        store.edit(["type":"effect","action":action]) { [weak self] error in
            if error == nil { self?.rename = nil }
            completion(error)
        }
    }
}

struct PaperFiles: ViewModifier {
    let store: EditorStore
    @ObservedObject var controller: PaperController
    func body(content: Content) -> some View {
        content.sheet(item:controller.texture == nil ? $controller.rename : Binding<PaperRename?>.constant(nil)) { draft in
            PaperNameForm(controller:controller,draft:draft).modifier(EditorPopupPresentation())
        }
            .sheet(item:$controller.texture,onDismiss:{ controller.textureClosed() }) { request in
                TextureDialog(store:store,controller:controller,request:request).modifier(EditorDialogPresentation(identifier:"texture-dialog",idealHeight:600))
            }
            .overlay(alignment:.top) {
                if controller.busy {
                    HStack { ProgressView(); Button(controller.common["cancel"].string) { controller.cancelWork() } }
                        .padding(8).background(.regularMaterial)
                        .accessibilityElement(children:.contain).accessibilityIdentifier("paper-library-progress")
                }
            }
    }
}

private struct PaperNameForm: View {
    @Environment(\.capyNativeCopy) private var copy
    @ObservedObject var controller: PaperController
    let draft: PaperRename
    @State private var name: String
    @State private var error: String?
    @State private var saving = false
    init(controller:PaperController,draft:PaperRename) {
        self.controller = controller; self.draft = draft; _name = State(initialValue:draft.action["name"].string)
    }
    var body: some View {
        VStack(alignment:.leading,spacing:12) {
            Text(copy["paper"]["rename_title"].string).font(.headline)
            TextField(controller.common["name"].string,text:$name).textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("paper-library-name").onSubmit(save)
            if let error { Text(error).foregroundStyle(.red).accessibilityIdentifier("paper-name-error") }
            HStack {
                Spacer()
                Button(controller.common["cancel"].string,role:.cancel) { controller.rename = nil }.keyboardShortcut(.cancelAction)
                Button(controller.copy["rename"].string,action:save).disabled(saving).keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("paper-name-save")
            }
        }.padding(20).frame(minWidth:320)
            .accessibilityElement(children:.contain).accessibilityIdentifier("paper-name-dialog")
    }
    private func save() {
        guard !saving else { return }
        saving = true
        controller.renamed(name) { error = $0; saving = false }
    }
}

struct PaperTextureButton: View {
    @ObservedObject var store: EditorStore
    let layer: UInt64
    let control: JSON
    var body: some View {
        let tile = control["paper"]["tiles"][Int(control["value"]["value"].uint)], label = control["label"].string
        Button { store.papers.choose(layer:layer) } label: {
            HStack { PaperPreview(previews:store.papers.previews,id:tile["id"].raw as? String,side:28); Text(tile["label"].string).lineLimit(1); Spacer(minLength:0) }
                .contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle()).help(label).accessibilityLabel(label)
            .accessibilityValue(tile["label"].string)
            .accessibilityIdentifier("property-" + control["key"].string)
    }
}

private struct TextureDialog: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    @ObservedObject var store: EditorStore
    @ObservedObject var controller: PaperController
    let request: PaperTexture
    @State private var started = false
    @State private var pending: String?
    @State private var width: CGFloat = 0
    @FocusState private var focused: Bool
    private var control: JSON {
        let properties = store.state["layer_properties"]
        guard store.state["document_file"]["epoch"].uint == request.epoch, properties["layer"].optionalUInt == request.layer else { return JSON() }
        return properties["controls"].array.first { !$0["paper"].isNull } ?? JSON()
    }
    private var ids: [String?] { control["paper"]["tiles"].array.map { $0["id"].raw as? String } }
    private var selected: Int { Int(control["value"]["value"].uint) }
    var body: some View {
        let control = self.control, tiles = control["paper"]["tiles"].array
        VStack(spacing:0) {
            ScrollViewReader { proxy in
                EditorScrollView {
                    VStack(spacing:16) {
                        EditorDialogTitle(text:nativeCopy["paper"]["choose"].string)
                        LazyVGrid(columns:[GridItem(.adaptive(minimum:102,maximum:102),spacing:12,alignment:.top)],spacing:14) {
                            ForEach(tiles.indices,id:\.self) { index in tile(tiles[index],index:index).id(index) }
                        }.frame(maxWidth:672)
                            .onGeometryChange(for:CGFloat.self) { $0.size.width } action: { width = $0 }
                            .focusable().focused($focused).focusEffectDisabled()
                            .onKeyPress(keys:[.leftArrow,.rightArrow,.upArrow,.downArrow]) { press in move(press.key) }
                    }.padding(20).frame(maxWidth:.infinity)
                }.onChange(of:started) { _, _ in reveal(proxy) }
                    .onChange(of:pending) { _, _ in reveal(proxy) }
            }
            EditorDialogFooter(label:nativeCopy["paper"]["use_texture"].string,identifiers:(cancel:"texture-cancel",apply:"texture-apply"),
                cancel:{ controller.closeTexture() },apply:useTexture) {
                let item = control["paper"]["import"]
                if !item.isNull {
                    Button(item["label"].string) { controller.closeTexture(then:item) }.buttonStyle(.bordered)
                        .accessibilityIdentifier("texture-import")
                }
                Spacer(minLength:8)
            }
        }.onChange(of:JSON([control["paper"]["tiles"].raw,control["value"].raw]).stableKey,initial:true) { _, _ in follow() }
            .onAppear { focused = true }
            .sheet(item:$controller.rename) { draft in PaperNameForm(controller:controller,draft:draft).modifier(EditorPopupPresentation()) }
    }
    private func tile(_ item: JSON, index: Int) -> some View {
        let id = item["id"].raw as? String, actions = item["actions"].array
        return TextureTile(previews:controller.previews,id:id,label:item["label"].string,pending:started && pending == id) { pending = id }
            .accessibilityIdentifier("texture-tile-\(index)")
            .contextMenu {
                ForEach(actions.indices,id:\.self) { action in
                    Button(actions[action]["label"].string) { controller.action(actions[action]) }
                }
            }
    }
    private func reveal(_ proxy: ScrollViewProxy) {
        if started, let index = ids.firstIndex(of:pending) { proxy.scrollTo(index) }
    }
    private func follow() {
        guard !control.isNull else { return }
        let ids = self.ids
        if !started || !ids.contains(pending) {
            pending = ids.indices.contains(selected) ? ids[selected] : nil
            started = true
        }
    }
    private func move(_ key: KeyEquivalent) -> KeyPress.Result {
        let ids = self.ids, columns = max(1,Int((width + 12) / 114))
        guard started, let index = ids.firstIndex(of:pending) else { return .ignored }
        let next = index + (key == .leftArrow ? -1 : key == .rightArrow ? 1 : key == .upArrow ? -columns : columns)
        if ids.indices.contains(next) { pending = ids[next] }
        return .handled
    }
    private func useTexture() {
        let control = self.control
        if started, !control.isNull, let index = ids.firstIndex(of:pending), index != selected {
            store.effect(request.layer,epoch:request.epoch,key:control["key"].string,
                action:["op":"set","value":["kind":"choice","value":index]]) { if let error = $0 { store.failure = error } }
        }
        controller.closeTexture()
    }
}

struct TextureTile: View {
    @Environment(\.editorPopupStore) private var store
    let previews: PaperPreviewCache
    let id: String?
    let label: String
    let pending: Bool
    var placeholder = "layer-paper-symbolic"
    let select: () -> Void
    @State private var hovering = false
    var body: some View {
        let palette = EditorPalette(source:store?.state["palette"] ?? JSON())
        Button(action:select) {
            VStack(spacing:6) {
                PaperPreview(previews:previews,id:id,side:96,placeholder:placeholder)
                    .background(palette["text"].opacity(0.06),in:SquircleShape.tile)
                    .padding(3)
                    .background { if hovering { SquircleShape.tile.fill(palette["text"].opacity(0.1)) } }
                    .overlay { if pending { SquircleShape.tile.strokeBorder(palette.accent,lineWidth:2) } }
                Text(label).font(.caption).multilineTextAlignment(.center).lineLimit(2).frame(width:102)
            }.contentShape(Rectangle())
        }.buttonStyle(.plain).onHover { hovering = $0 }
            .help(label).accessibilityLabel(label).accessibilityAddTraits(pending ? .isSelected : [])
    }
}
