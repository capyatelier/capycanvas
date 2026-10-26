import SwiftUI
#if os(iOS)
import UIKit
#endif

struct NewDrawingPresentation: ViewModifier {
    @ObservedObject var store: EditorStore
    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(get: { !store.state["new_drawing"].isNull }, set: {
            if !$0 && !store.state["new_drawing"].isNull { store.projectFiles.newDrawing(["op": "cancel"]) }
        }), onDismiss: { store.focusCanvas?() }) {
            NewDrawingDialog(store: store)
                .frame(minWidth: 340, idealWidth: 720, maxWidth: 760, minHeight: 440, idealHeight: 540, maxHeight: 680)
                .accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-dialog")
                .modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
        }
    }
}

private extension EditorPalette {
    var newDrawingField: Color { self["text"].opacity(0.07) }
}

private struct NewDrawingDialog: View {
    @ObservedObject var store: EditorStore
    @State private var error: String?
    @State private var admissions = NumericAdmissions()
    @State private var width: CGFloat = 720
    @State private var name = ""
    @State private var query = ""
    @State private var presetName = ""
    @FocusState private var focused: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var resolutions = false
    @State private var importing = false
    @State private var paperColor: ColorEditorSession?
    private static let reliefField = "new-drawing-paper-relief"
    private var view: JSON { store.state["new_drawing"] }
    private var copy: JSON { view["copy"] }
    private var page: JSON { view["page"] }
    private var drawer: JSON { view["drawer"] }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var narrow: Bool { width < 640 }

    var body: some View {
        ZStack(alignment: .bottom) {
            main.disabled(!drawer.isNull).accessibilityHidden(!drawer.isNull)
            if !drawer.isNull {
                opened.frame(maxWidth: .infinity, maxHeight: .infinity).background(palette["settings"])
                    .transition(.move(edge: .bottom))
            }
        }.clipped()
            .animation(reduceMotion ? nil : .timingCurve(0.215, 0.61, 0.355, 1, duration: 0.25), value: drawer["kind"].string)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
            .onAppear { name = view["name"].string }
            .onChange(of: name) { _, text in if text != view["name"].string { send(["op": "name", "text": text]) } }
            .onChange(of: drawer["kind"].string, initial: true) { _, kind in follow(kind) }
            .onChange(of: drawer["save"].isNull, initial: true) { _, closed in if !closed { presetName = drawer["save"]["name"].string } }
            .onChange(of: drawer["query"].string) { _, shared in if shared.isEmpty { query = "" } }
            .onDisappear { closePaperColor() }
    }

    private func send(_ action: [String: Any]) {
        store.projectFiles.newDrawing(action) { error = $0 }
    }
    @State private var previousDrawer = ""
    private func follow(_ kind: String) {
        let previous = previousDrawer
        previousDrawer = kind
        if kind == "presets" { query = drawer["query"].string; focused = "search" }
        else if kind.isEmpty { focused = previous }
        if kind == "paper_color" {
            if paperColor == nil {
                paperColor = ColorEditorSession(controller: nil, store: store, colors: store.displayColors, value: drawer["color"], opaque: true) { _, _ in }
            }
        } else { closePaperColor() }
    }
    private func closePaperColor() {
        guard let session = paperColor else { return }
        paperColor = nil
        session.close(apply: false)
    }
    private func create() {
        admit {
            if name != view["name"].string { send(["op": "name", "text": name]) }
            send(["op": "create"])
        }
    }
    private func usePaper() { admit(only: Self.reliefField) { send(["op": "use_paper"]) } }
    private func admit(only key: String? = nil, then action: @escaping () -> Void) {
        #if os(iOS)
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
        #endif
        DispatchQueue.main.async {
            guard admissions.finish(only: key) else { return }
            action()
        }
    }

    private var main: some View {
        VStack(spacing: 0) {
            EditorScrollView {
                VStack(spacing: 14) {
                    EditorDialogTitle(text: copy["title"].string)
                    if narrow {
                        VStack(spacing: 14) { preview; form }
                    } else {
                        HStack(alignment: .top, spacing: 24) { preview; form }
                    }
                }.padding(20).frame(maxWidth: .infinity)
            }
            if let error {
                Text(error).font(.caption).foregroundStyle(.red).frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 20).padding(.bottom, 6).accessibilityIdentifier("new-drawing-error")
            }
            EditorDialogFooter(label: copy["create"].string, identifiers: (cancel: "new-drawing-cancel", apply: "new-drawing-create"),
                blocked: !view["can_create"].bool, cancel: { send(["op": "cancel"]) }, apply: create) {
                Button { send(["op": "presets"]) } label: {
                    HStack(spacing: 6) { Text(copy["presets"].string); SharedIcon(name: "chevron-down", size: 12).rotationEffect(.degrees(180)) }
                }.buttonStyle(.bordered).accessibilityIdentifier("new-drawing-presets-button").focused($focused, equals: "presets")
                Spacer(minLength: 8)
            }
        }
    }

    private var preview: some View {
        let bounds = narrow ? CGSize(width: min(348, max(120, width - 40)), height: 140) : CGSize(width: 232, height: 250)
        return VStack(spacing: 8) {
            Button { send(["op": "presets"]) } label: {
                NewDrawingSheet(previews: store.papers.previews, sheet: view["sheet"],
                    fit: CGSize(width: bounds.width - 36, height: bounds.height - 36), grain: 40)
                    .frame(width: bounds.width, height: bounds.height)
                    .background(palette["text"].opacity(0.06), in: SquircleShape.surface).contentShape(Rectangle())
            }.buttonStyle(.plain).help(copy["presets"].string).accessibilityLabel(copy["presets"].string)
                .accessibilityIdentifier("new-drawing-preview")
            VStack(spacing: 2) {
                HStack(spacing: 4) {
                    Text(page["preset"].string).fontWeight(.semibold).lineLimit(1)
                    Text("· " + page["orientation_label"].string).foregroundStyle(.secondary).lineLimit(1)
                }.accessibilityElement(children: .combine).accessibilityIdentifier("new-drawing-caption")
                Text(page["capacity"].string).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    .help(page["memory"].string).accessibilityIdentifier("new-drawing-capacity")
            }
        }.frame(width: bounds.width)
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Text(copy["name"].string)
                TextField(view["untitled"].string, text: $name).textFieldStyle(.plain).autocorrectionDisabled()
                    .padding(.horizontal, 10).frame(height: 34).background(palette.newDrawingField, in: SquircleShape.control)
                    #if os(iOS)
                    .onSubmit(create)
                    #endif
                    .accessibilityLabel(copy["name"].string).accessibilityIdentifier("new-drawing-name")
            }
            section(copy["size"].string) {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 6) {
                        HStack(spacing: 0) { dimension(0); link; dimension(1) }
                        choice(copy["unit"].string, view["units"].array, value: view["setup"]["unit"], identifier: "new-drawing-unit") { ["op": "unit", "unit": $0.raw] }
                            .fixedSize()
                        Spacer(minLength: 0)
                        orientation
                    }
                    HStack(spacing: 6) {
                        Text(copy["resolution"].string).padding(.trailing, 4)
                        resolution
                        Text(page["size"].string).font(.caption).foregroundStyle(page["problem"].bool ? Color.red : Color.secondary)
                            .lineLimit(2).padding(.leading, 10).accessibilityIdentifier("new-drawing-size")
                        Spacer(minLength: 0)
                    }
                }
            }
            section(copy["color"].string) {
                HStack(alignment: .top, spacing: 6) {
                    captioned(copy["space"].string) {
                        choice(copy["space"].string, view["spaces"].array, value: view["setup"]["color"]["space"], identifier: "new-drawing-space") { ["op": "space", "space": $0.raw] }
                    }
                    captioned(copy["depth"].string) {
                        choice(copy["depth"].string, view["depths"].array, value: view["setup"]["color"]["depth"], identifier: "new-drawing-depth") { ["op": "depth", "depth": $0.raw] }
                    }
                    let note = view["blending_note"]
                    captioned(copy["blending"].string) {
                        choice(copy["blending"].string, view["blendings"].array, value: view["setup"]["blend_space"], identifier: "new-drawing-blending") { ["op": "blending", "blending": $0.raw] }
                            .disabled(!note.isNull).opacity(note.isNull ? 1 : 0.4)
                    }.help(note.string).accessibilityHint(note.string)
                }
            }
            section(copy["paper"].string) {
                HStack(spacing: 6) {
                    paperTile("new-drawing-texture", label: page["texture"].string, detail: page["relief"].raw as? String,
                        help: copy["paper"].string, enabled: true, op: "paper") {
                        PaperPreview(previews: store.papers.previews, id: view["setup"]["paper"]["texture"].raw as? String, side: 36, placeholder: "none")
                    }
                    paperTile("new-drawing-paper-color", label: page["paper_color"].string, help: copy["paper_color"].string,
                        enabled: view["paper_color_enabled"].bool, op: "paper_color") {
                        ColorSwatch(rgba: view["sheet"]["shown"].bool ? ColorUI.preview(view["sheet"]["color"])["rgba"] : JSON([0, 0, 0, 0]),
                            shape: SquircleShape.tile).frame(width: 36, height: 36)
                    }
                }
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }

    private func section<Content: View>(_ heading: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(heading).fontWeight(.semibold).accessibilityAddTraits(.isHeader)
            content()
        }
    }

    private func captioned<Content: View>(_ caption: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(caption).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            content()
        }.frame(maxWidth: .infinity, alignment: .leading)
    }

    private func choice(_ label: String, _ choices: [JSON], value: JSON, identifier: String,
        action: @escaping (JSON) -> [String: Any]) -> some View {
        let key = value.stableKey
        return EditorChoice(label: label, options: choices.map { $0["label"].string },
            selected: choices.firstIndex { $0["value"].stableKey == key } ?? -1, identifier: identifier,
            background: palette.newDrawingField, height: 34) { index in
            if choices.indices.contains(index) { send(action(choices[index]["value"])) }
        }
    }

    private func dimension(_ axis: Int) -> some View {
        let op = axis == 0 ? "width" : "height"
        return NumberControl(store: store, label: copy[op + "_name"].string, value: view["values"][axis].number, control: view["numeric"],
            identifier: "new-drawing-" + op, affixes: (copy[op].string, ""),
            registerAdmission: { key, change in admissions.receive(key, change) }) { value, completion in
            store.projectFiles.newDrawing(["op": op, "value": value]) { failure in error = failure; completion(failure) }
        }.id(view["numeric"].stableKey).frame(width: 96, height: 34).background(palette.newDrawingField, in: SquircleShape.control)
    }

    private var link: some View {
        let linked = view["linked"].bool
        return Button { send(["op": "linked", "linked": !linked]) } label: {
            SharedIcon(name: linked ? "link" : "unlink").frame(width: 28, height: 34).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle(active: linked)).help(copy["link"].string).accessibilityLabel(copy["link"].string)
            .accessibilityAddTraits(linked ? .isSelected : []).accessibilityIdentifier("new-drawing-link")
    }

    private var orientation: some View {
        HStack(spacing: 2) {
            ForEach(["portrait", "landscape"], id: \.self) { side in
                IconTile(icon: side, label: copy[side].string, selected: page["orientation"].string == side) {
                    send(["op": "orientation", "orientation": side])
                }.frame(width: 30, height: 30).accessibilityIdentifier("new-drawing-" + side)
            }
        }.padding(2).background(palette.newDrawingField, in: SquircleShape.control).fixedSize()
            .accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-orientation")
    }

    private var resolution: some View {
        HStack(spacing: 0) {
            NumberControl(store: store, label: copy["resolution"].string, value: view["setup"]["resolution"].number,
                control: view["resolution_numeric"], identifier: "new-drawing-resolution", affixes: ("", copy["resolution_unit"].string),
                registerAdmission: { key, change in admissions.receive(key, change) }) { value, completion in
                store.projectFiles.newDrawing(["op": "resolution", "value": value]) { failure in error = failure; completion(failure) }
            }.frame(width: 86)
            Button { resolutions = true } label: {
                SharedIcon(name: "chevron-down", size: 12).frame(width: 26, height: 34).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).help(copy["common_resolutions"].string)
                .accessibilityLabel(copy["common_resolutions"].string).accessibilityIdentifier("new-drawing-resolution-choices")
        }.frame(height: 34).background(palette.newDrawingField, in: SquircleShape.control)
            .editorPopover(isPresented: $resolutions) {
                EditorActionMenu(model: AppleContextMenu(JSON(["sections": [resolutionItems]])) {
                    send(["op": "resolution", "value": $0["value"].number])
                }, width: 240, identifier: "new-drawing-resolution-menu") { resolutions = false }
            }
    }

    private var resolutionItems: [[String: Any]] {
        let choices = view["resolutions"].array
        return choices.indices.map { index -> [String: Any] in
            let choice = choices[index]
            return ["label": choice["label"].raw, "hint": choice["purpose"].raw, "enabled": choice["enabled"].bool,
                "selected": choice["selected"].bool, "identifier": "new-drawing-resolution-\(index)", "action": ["value": choice["value"].raw]]
        }
    }

    private func paperTile<Thumbnail: View>(_ identifier: String, label: String, detail: String? = nil, help: String, enabled: Bool, op: String,
        @ViewBuilder thumbnail: () -> Thumbnail) -> some View {
        Button { send(["op": op]) } label: {
            HStack(spacing: 10) {
                thumbnail()
                VStack(alignment: .leading, spacing: 1) {
                    Text(label).fontWeight(.semibold).lineLimit(1)
                    if let detail { Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                }
                Spacer(minLength: 0)
            }.padding(4).padding(.trailing, 6).frame(maxWidth: .infinity, minHeight: 44)
                .background(palette.newDrawingField, in: SquircleShape.control).contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(!enabled).opacity(enabled ? 1 : 0.4)
            .help(help).accessibilityLabel(help).accessibilityValue([label, detail].compactMap { $0 }.joined(separator: ", "))
            .focused($focused, equals: identifier == "new-drawing-texture" ? "paper" : "paper_color")
            .accessibilityIdentifier(identifier)
    }

    @ViewBuilder private var opened: some View {
        switch drawer["kind"].string {
        case "presets": presets
        case "paper": papers
        case "paper_color":
            if let paperColor {
                NewDrawingPaperColor(store: store, session: paperColor, copy: copy, drawer: drawer, send: send)
            }
        default: EmptyView()
        }
    }

    private var presets: some View {
        let tiles = drawer["tiles"].array, tabs = drawer["tabs"].array
        return VStack(spacing: 0) {
            HStack(spacing: 8) {
                SharedIcon(name: "search").opacity(0.6)
                TextField(copy["search"].string, text: $query).textFieldStyle(.plain).autocorrectionDisabled()
                    .onChange(of: query) { _, text in if text != drawer["query"].string { send(["op": "search", "query": text]) } }
                    .accessibilityLabel(copy["search"].string).accessibilityIdentifier("new-drawing-search").focused($focused, equals: "search")
            }.padding(.horizontal, 10).frame(height: 34).background(palette.newDrawingField, in: SquircleShape.control)
                .padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 10)
            EditorScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ConfigurationFlow(spacing: 4) {
                        ForEach(tabs.indices, id: \.self) { index in pill(tabs[index], index: index) }
                    }.disabled(!drawer["tabs_enabled"].bool).opacity(drawer["tabs_enabled"].bool ? 1 : 0.4)
                        .accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-tabs")
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 128, maximum: 128), spacing: 10, alignment: .top)], spacing: 12) {
                        ForEach(tiles.indices, id: \.self) { index in presetTile(tiles[index], index: index) }
                    }.accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-presets")
                    if !drawer["empty"].isNull {
                        Text(drawer["empty"].string).font(.caption).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("new-drawing-empty")
                    }
                }.padding(.horizontal, 20).padding(.bottom, 20).frame(maxWidth: .infinity, alignment: .leading)
            }
            EditorLine(color: palette.settingsBorder)
            HStack(spacing: 10) {
                summary
                Spacer(minLength: 8)
                ZStack {
                    if drawer["save"].isNull { browserActions.transition(.opacity) }
                    else { saveRow(drawer["save"]).transition(.opacity) }
                }.animation(reduceMotion ? nil : .easeInOut(duration: 0.2), value: drawer["save"].isNull)
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-browser")
            .onKeyPress(.escape) {
                guard !NativeTextContext.composing else { return .ignored }
                send(["op": "back"])
                return .handled
            }
    }

    private func pill(_ tab: JSON, index: Int) -> some View {
        let selected = tab["selected"].bool
        return Button { send(["op": "tab", "index": index]) } label: {
            Text(tab["label"].string).lineLimit(1).padding(.horizontal, 10).frame(height: 28)
                .background(selected ? palette.active : palette.newDrawingField, in: SquircleShape.control).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityAddTraits(selected ? .isSelected : []).accessibilityIdentifier("new-drawing-tab-\(index)")
    }

    @ViewBuilder private func presetTile(_ tile: JSON, index: Int) -> some View {
        let selected = tile["selected"].bool, name = tile["name"].string
        let face = Button { if !selected { send(["op": "pick_preset", "index": index]) } } label: {
            VStack(spacing: 4) {
                NewDrawingSheet(previews: store.papers.previews, sheet: tile["sheet"], fit: CGSize(width: 74, height: 74), grain: 14)
                    .frame(width: 116, height: 86)
                    .background { if selected { SquircleShape.control.fill(palette["text"].opacity(0.08)) } }
                    .overlay { if selected { SquircleShape.control.strokeBorder(palette.accent, lineWidth: 2) } }
                VStack(spacing: 1) {
                    Text(name).lineLimit(2)
                    Text(tile["dimensions"].string).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    Text(tile["details"].string).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }.multilineTextAlignment(.center)
            }.frame(width: 128).opacity(tile["enabled"].bool ? 1 : 0.4).contentShape(Rectangle())
        }.buttonStyle(.plain)
            .simultaneousGesture(TapGesture(count: 2).onEnded {
                send(["op": "pick_preset", "index": index]); send(["op": "use_preset"])
            })
            .help(name + "\n" + tile["dimensions"].string + " · " + tile["details"].string)
            .accessibilityLabel(name).accessibilityValue(tile["dimensions"].string)
            .accessibilityAddTraits(selected ? .isSelected : []).accessibilityIdentifier("new-drawing-preset-\(index)")
        if tile["own"].bool {
            face.contextMenu {
                Button(copy["rename"].string) { send(["op": "rename_preset", "index": index]) }
                    .accessibilityIdentifier("new-drawing-rename-preset-\(index)")
                Button(copy["delete"].string, role: .destructive) { send(["op": "delete_preset", "index": index]) }
                    .accessibilityIdentifier("new-drawing-delete-preset-\(index)")
            }
        } else { face }
    }

    private var summary: some View {
        let summary = drawer["summary"]
        return HStack(spacing: 10) {
            NewDrawingSheet(previews: store.papers.previews, sheet: drawer["summary_sheet"], fit: CGSize(width: 34, height: 34), grain: 14)
                .frame(width: 36, height: 36)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Text(summary["name"].string).fontWeight(.semibold).lineLimit(1).accessibilityIdentifier("new-drawing-summary-name")
                    Text(summary["size"].string).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        .accessibilityIdentifier("new-drawing-summary-size")
                }
                Text(summary["details"].string).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    .accessibilityIdentifier("new-drawing-summary-details")
            }
        }.layoutPriority(-1)
    }

    private var browserActions: some View {
        HStack(spacing: 6) {
            Button(copy["cancel"].string) { send(["op": "close_drawer"]) }.buttonStyle(.bordered)
                .accessibilityIdentifier("new-drawing-browser-cancel")
            Button(drawer["save_label"].string) { send(["op": "save_as"]) }.buttonStyle(.bordered)
                .accessibilityIdentifier("new-drawing-save-as")
            Button(copy["select"].string) { send(["op": "use_preset"]) }.buttonStyle(.borderedProminent).tint(palette.accent)
                .disabled(!drawer["can_select"].bool).keyboardShortcut(.defaultAction).accessibilityIdentifier("new-drawing-select")
        }.fixedSize()
    }

    private func saveRow(_ save: JSON) -> some View {
        HStack(spacing: 6) {
            TextField(copy["preset_name"].string, text: $presetName).textFieldStyle(.plain).autocorrectionDisabled()
                .padding(.horizontal, 8).frame(width: 150, height: 34).background(palette.newDrawingField, in: SquircleShape.control)
                .overlay { if !save["can_save"].bool { SquircleShape.control.stroke(Color.red, lineWidth: 1) } }
                .onChange(of: presetName) { _, text in if text != drawer["save"]["name"].string { send(["op": "preset_name", "text": text]) } }
                .onSubmit { send(["op": "save_preset"]) }
                .accessibilityLabel(copy["preset_name"].string).accessibilityIdentifier("new-drawing-save-name").focused($focused, equals: "save")
                .onAppear { focused = "save" }
            choice(copy["preset_category"].string, save["groups"].array, value: save["group"], identifier: "new-drawing-save-group") {
                ["op": "preset_group", "group": $0.raw]
            }.fixedSize().help(copy["preset_category"].string)
            Button(copy["cancel"].string) { send(["op": "cancel_save"]) }.buttonStyle(.bordered)
                .keyboardShortcut(.cancelAction).accessibilityIdentifier("new-drawing-save-cancel")
            Button(save["confirm"].string) { send(["op": "save_preset"]) }.buttonStyle(.borderedProminent).tint(palette.accent)
                .disabled(!save["can_save"].bool).keyboardShortcut(.defaultAction).accessibilityIdentifier("new-drawing-save-confirm")
        }.fixedSize()
    }

    private var papers: some View {
        let tiles = drawer["tiles"].array
        return VStack(spacing: 0) {
            EditorScrollView {
                VStack(spacing: 16) {
                    EditorDialogTitle(text: copy["paper"].string)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 102, maximum: 102), spacing: 12, alignment: .top)], spacing: 14) {
                        ForEach(tiles.indices, id: \.self) { index in paperChoice(tiles[index], index: index) }
                        TextureTile(previews: store.papers.previews, id: nil, label: copy["import"].string, pending: false,
                            placeholder: "plus", select: importPaper)
                            .disabled(importing).accessibilityIdentifier("new-drawing-paper-import")
                    }.frame(maxWidth: 672).accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-papers")
                }.padding(20).frame(maxWidth: .infinity)
            }
            EditorDialogFooter(label: copy["use_texture"].string, identifiers: (cancel: "new-drawing-paper-cancel", apply: "new-drawing-paper-use"),
                cancel: { _ = admissions.finish(true, only: Self.reliefField); send(["op": "close_drawer"]) }, apply: usePaper) {
                reliefRow(drawer["relief"])
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-paper-drawer")
    }

    private func reliefRow(_ relief: JSON) -> some View {
        let enabled = relief["enabled"].bool
        return HStack(spacing: 10) {
            Text(copy["relief"].string).lineLimit(1).truncationMode(.tail).help(copy["relief"].string)
            NumberControl(store: store, label: copy["relief"].string, value: relief["value"].number, control: relief["numeric"],
                identifier: Self.reliefField, affixes: ("", copy["relief_unit"].string),
                registerAdmission: { key, change in admissions.receive(key, change) }) { value, completion in
                store.projectFiles.newDrawing(["op": "paper_relief", "value": value]) { failure in error = failure; completion(failure) }
            }.frame(width: 62).padding(.trailing, 10).frame(height: 34).background(palette.newDrawingField, in: SquircleShape.control)
        }.disabled(!enabled).opacity(enabled ? 1 : 0.4).padding(.trailing, 8)
            .frame(maxWidth: .infinity, alignment: .leading).layoutPriority(-1)
    }

    private func paperChoice(_ tile: JSON, index: Int) -> some View {
        let id = tile["id"].raw as? String, selected = tile["selected"].bool
        let pick: [String: Any] = ["op": "pick_paper", "id": id ?? NSNull()]
        return TextureTile(previews: store.papers.previews, id: id, label: tile["label"].string, pending: selected,
            placeholder: id == nil ? "none" : "layer-paper-symbolic") { if !selected { send(pick) } }
            .simultaneousGesture(TapGesture(count: 2).onEnded { send(pick); usePaper() })
            .accessibilityIdentifier("new-drawing-paper-tile-\(index)")
    }

    private func importPaper() {
        importing = true
        store.papers.importToLibrary { failure in
            importing = false
            if let failure { error = failure }
        }
    }
}

private struct NewDrawingPaperColor: View {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    @ObservedObject var store: EditorStore
    @ObservedObject var session: ColorEditorSession
    let copy: JSON
    let drawer: JSON
    let send: ([String: Any]) -> Void
    @State private var drafts: [String: String] = [:]
    @State private var sent: String?
    @FocusState private var focused: String?
    private var transparent: Bool { drawer["transparent"].bool }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        VStack(spacing: 0) {
            EditorScrollView {
                VStack(spacing: 16) {
                    EditorDialogTitle(text: copy["paper_color"].string)
                    ColorEditorForm(session: session, drafts: $drafts, focus: $focused, tools: false,
                        transparent: transparent ? copy["transparent"].string : nil, accessory: AnyView(mode), below: AnyView(colors))
                }.padding(20).frame(maxWidth: .infinity)
            }
            EditorDialogFooter(label: copy["use_color"].string, identifiers: (cancel: "new-drawing-color-cancel", apply: "new-drawing-color-use"),
                blocked: session.blocked, cancel: { send(["op": "close_drawer"]) }, apply: use) { Spacer(minLength: 8) }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-color-drawer")
            .onAppear { if sent == nil { sent = session.view["value"].stableKey } }
            .onChange(of: session.view["value"].stableKey) { _, _ in report() }
            .onChange(of: focused) { previous, _ in if let previous { ColorEditorForm.commit(previous, drafts: $drafts, session: session) } }
            .onChange(of: interfaceLanguage) { _, _ in session.relocalize() }
    }
    private func report() {
        let value = session.view["value"]
        guard !value.isNull, value.stableKey != sent else { return }
        sent = value.stableKey
        send(["op": "paper_color_value", "color": value.raw])
    }
    private func use() {
        if let focused { ColorEditorForm.commit(focused, drafts: $drafts, session: session) }
        guard !session.blocked else { return }
        report()
        send(["op": "use_paper_color"])
    }
    private var mode: some View {
        HStack(spacing: 2) {
            modeButton("solid", label: copy["color"].string, selected: !transparent, identifier: "new-drawing-color-mode-color") {
                send(["op": "transparent", "transparent": false])
            }
            modeButton("transparent", label: copy["transparent"].string, selected: transparent, identifier: "new-drawing-color-mode-transparent") {
                send(["op": "transparent", "transparent": true])
            }
        }.padding(2).background(palette.newDrawingField, in: SquircleShape.control).fixedSize()
            .accessibilityElement(children: .contain).accessibilityLabel(copy["sheet_mode"].string)
            .accessibilityIdentifier("new-drawing-color-mode")
    }
    private func modeButton(_ icon: String, label: String, selected: Bool, identifier: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 5) { SharedIcon(name: icon, size: 14); Text(label).lineLimit(1) }
                .padding(.horizontal, 10).frame(height: 30).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle(selected: selected))
            .accessibilityAddTraits(selected ? .isSelected : []).accessibilityIdentifier(identifier)
    }
    private var colors: some View {
        let choices = drawer["palette"].array
        let previews = ColorUI.resolve(["type": "preview", "colors": choices.map { $0["color"].raw }, "display_space": "DisplayP3"])
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(copy["paper_colors"].string)
                Text(drawer["count"].string).foregroundStyle(.secondary)
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 44, maximum: 44), spacing: 10)], alignment: .leading, spacing: 10) {
                ForEach(choices.indices, id: \.self) { index in swatch(choices[index], rgba: previews[index]["rgba"], index: index) }
                Button { send(["op": "add_paper_color"]) } label: {
                    SharedIcon(name: "plus").frame(width: 44, height: 44).contentShape(Rectangle())
                }.buttonStyle(EditorControlButtonStyle()).disabled(!drawer["can_add"].bool).opacity(drawer["can_add"].bool ? 1 : 0.36)
                    .help(copy["add_paper_color"].string).accessibilityLabel(copy["add_paper_color"].string)
                    .accessibilityIdentifier("new-drawing-add-paper-color")
            }
        }.frame(maxWidth: 420, alignment: .leading)
            .accessibilityElement(children: .contain).accessibilityIdentifier("new-drawing-paper-colors")
    }
    private func swatch(_ choice: JSON, rgba: JSON, index: Int) -> some View {
        let selected = choice["selected"].bool
        return Button { session.act(["op": "color", "color": choice["color"].raw]) } label: {
            ColorSwatch(rgba: rgba, shape: SquircleShape.control).frame(width: 40, height: 40).padding(2)
                .overlay { if selected { SquircleShape.control.strokeBorder(palette.accent, lineWidth: 2) } }
                .contentShape(Rectangle())
        }.buttonStyle(.plain).help(choice["name"].string).accessibilityLabel(choice["name"].string)
            .accessibilityAddTraits(selected ? .isSelected : []).accessibilityIdentifier("new-drawing-paper-color-\(index)")
            .contextMenu {
                Button(copy["delete"].string, role: .destructive) { send(["op": "remove_paper_color", "index": index]) }
                    .accessibilityIdentifier("new-drawing-remove-paper-color-\(index)")
            }
    }
}

struct NewDrawingSheet: View {
    @ObservedObject var previews: PaperPreviewCache
    let sheet: JSON
    let fit: CGSize
    let grain: CGFloat
    var body: some View {
        let width = CGFloat(max(1, sheet["extent"][0].number)), height = CGFloat(max(1, sheet["extent"][1].number))
        let scale = min(min(fit.width, fit.height) / max(width, height), fit.width / width, fit.height / height)
        let shown = sheet["shown"].bool, texture = sheet["texture"].raw as? String
        ZStack {
            ColorSwatch(rgba: shown ? ColorUI.preview(sheet["color"])["rgba"] : JSON([0, 0, 0, 0]))
            if let texture, let image = previews.grains[texture] {
                Image(decorative: image, scale: CGFloat(image.width) / grain).resizable(resizingMode: .tile)
                    .opacity(shown ? 1 : 0.35)
            }
        }.compositingGroup()
            .frame(width: max(4, (width * scale).rounded()), height: max(4, (height * scale).rounded()))
            .clipShape(RoundedRectangle(cornerRadius: 2))
            .shadow(color: .black.opacity(0.18), radius: 2, y: 1)
            .accessibilityHidden(true)
            .task(id: texture) { if let texture { await previews.load(texture) } }
    }
}
