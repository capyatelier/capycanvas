import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

struct ManagedColorButton: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.editorPopupStore) private var store
    let label: String
    let identifier: String
    let value: JSON
    let colors: JSON
    var viewing = JSON()
    var opaque = false
    var titled = true
    var swatchWidth: CGFloat = 48
    let change: (JSON) -> Void
    var body: some View {
        let preview = ColorUI.preview(value)
        HStack {
            if titled { Text(label).frame(maxWidth: .infinity, alignment: .leading) }
            Button {
                store?.colorEditing.open(colors: colors, value: value, opaque: opaque, viewing: viewing) { color, _ in change(color) }
            } label: {
                ColorSwatch(rgba: preview["rgba"], shape: .control).frame(width: swatchWidth, height: 28)
                    .overlay(SquircleShape.control.stroke(.primary.opacity(0.3), lineWidth: 1))
            }.buttonStyle(.plain).accessibilityLabel(label).accessibilityIdentifier(identifier + "-color")
                .help(preview["in_gamut"].bool ? label : nativeCopy["color"]["outside_p3"].string)
        }
    }
}

struct ColorEditingPresentation: ViewModifier {
    @ObservedObject var controller: ColorEditingController
    func body(content: Content) -> some View {
        content.overlay { if let session = controller.session { ColorEditingHost(session: session) } }
    }
}

private struct ColorEditingHost: View {
    @Environment(\.editorPopupStore) private var store
    @ObservedObject var session: ColorEditorSession
    var body: some View {
        Color.clear.allowsHitTesting(false)
            .sheet(isPresented: Binding(get: { !session.picking }, set: { if !$0 && !session.picking { session.close(apply: false) } }), onDismiss: { store?.focusCanvas?() }) {
                ColorEditor(session: session).presentationSizing(.fitted).modifier(EditorPopupPresentation())
            }
            .overlay(alignment: .topLeading) { if session.picking, let store { ColorPickStrip(store: store, session: session) } }
    }
}

struct ColorEditor: View {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.capyCommonCopy) private var commonCopy
    @Environment(\.editorPopupStore) private var store
    @Environment(\.displayScale) private var displayScale
    @ObservedObject var session: ColorEditorSession
    @State private var drafts: [String: String] = [:]
    @State private var sheetOpen = false
    @State private var search = ""
    @State private var copied: String?
    @FocusState private var focused: String?
    private var view: JSON { session.view }
    private var copy: JSON { nativeCopy["color"] }
    private var palette: EditorPalette { EditorPalette(source: store?.state["palette"] ?? JSON()) }
    var body: some View {
        VStack(spacing: 0) {
            ZStack(alignment: .bottom) {
                EditorScrollView {
                    VStack(spacing: 16) {
                        Text(copy["edit"].string).font(.headline).frame(maxWidth: .infinity)
                        ColorEditorLayout {
                            head
                            VStack(spacing: 12) { ColorEditorWheel(session: session, side: 236); shapes }
                            VStack(alignment: .leading, spacing: 18) { rows; status }
                        }
                    }.padding(20).frame(maxWidth: .infinity).disabled(sheetOpen)
                }
                if sheetOpen { swatchSheet.transition(.move(edge: .bottom)) }
            }.clipped()
            Divider()
            footer.padding(.horizontal, 20).padding(.vertical, 12)
        }.frame(minWidth: 380, idealWidth: 800, maxWidth: 860, minHeight: 420, idealHeight: 500, maxHeight: 620)
            .onChange(of: interfaceLanguage) { _, _ in session.relocalize(); session.refreshSwatches(sheetOpen: sheetOpen) }
            .onChange(of: focused) { previous, _ in if let previous { commit(previous) } }
            #if os(macOS)
            .onPasteCommand(of: [.utf8PlainText]) { providers in
                _ = providers.first?.loadObject(ofClass: NSString.self) { text, _ in
                    guard let text = text as? String else { return }
                    DispatchQueue.main.async { session.act(["op": "text", "text": text]) }
                }
            }
            .onCopyCommand { [NSItemProvider(object: view["hex"].string as NSString)] }
            #else
            .background(ColorClipboardKeys(editing: focused != nil,
                copy: { Self.copy(view["hex"].string) },
                paste: { if let text = UIPasteboard.general.string { session.act(["op": "text", "text": text]) } }))
            #endif
    }
    private var shapes: some View {
        HStack(spacing: 2) {
            ForEach(view["shapes"].array.indices, id: \.self) { index in
                let choice = view["shapes"][index]
                Button { session.act(["op": "wheel", "action": ["op": "shape", "shape": choice["shape"].raw]]) } label: {
                    HStack(spacing: 4) { SharedIcon(name: "color-" + choice["shape"].string, size: 16); Text(choice["label"].string) }
                        .padding(.horizontal, 10).frame(height: 28).contentShape(Rectangle())
                        .background { if choice["selected"].bool { Capsule().fill(palette.accent.opacity(0.22)) } }
                }.buttonStyle(.plain).help(choice["name"].string).accessibilityLabel(choice["name"].string)
                    .accessibilityAddTraits(choice["selected"].bool ? .isSelected : [])
                    .accessibilityIdentifier("color-editor-shape-" + choice["shape"].string)
            }
        }.padding(2).background(Capsule().fill(palette["input"])).fixedSize()
    }
    private var head: some View {
        HStack(alignment: .center, spacing: 12) {
            HStack(spacing: 0) {
                pairCell(session.viewing["hdr"].bool ? view["current_value"] : JSON(), rgba: view["current"]["rgba"])
                    .contentShape(Rectangle()).onTapGesture { session.act(["op": "revert"]) }
                    .accessibilityLabel(copy["current"].string).accessibilityAddTraits(.isButton).accessibilityIdentifier("color-current")
                pairCell(session.viewing["hdr"].bool ? view["value"] : JSON(), rgba: view["new"]["rgba"])
                    .accessibilityLabel(copy["new"].string).accessibilityIdentifier("color-new")
            }.clipShape(SquircleShape.control)
                .overlay(alignment: .bottom) {
                    HStack(spacing: 0) {
                        Text(copy["current"].string).frame(width: 54)
                        Text(copy["new"].string).frame(width: 54)
                    }.font(.caption).foregroundStyle(.secondary).lineLimit(1).offset(y: 18)
                }
            Button { session.startPicking(touchOffset: 44 * displayScale) } label: {
                SharedIcon(name: "eyedropper").frame(width: 48, height: 48).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).help(copy["pick_canvas"].string)
                .accessibilityLabel(copy["pick_canvas"].string).accessibilityIdentifier("color-pick")
            VStack(alignment: .leading, spacing: 2) {
                if !view["hex_note"].isNull {
                    Text(view["hex_note"]["text"].string).font(.caption).foregroundStyle(.secondary).help(view["hex_note"]["tip"].string)
                }
                HStack(spacing: 4) {
                    ColorValueField(name: "hex", shown: JSON(["text": view["hex"].raw, "edit": view["hex"].raw, "name": copy["hex"].raw]),
                        width: 120, large: true, draft: draft("hex"), focus: $focused, session: session)
                    copyButton("hex", text: view["hex"].string)
                }
            }
        }.padding(.bottom, 18)
    }
    private func pairCell(_ hdr: JSON, rgba: JSON) -> some View {
        Group { if hdr.isNull { ColorSwatch(rgba: rgba) } else { HDRColorSwatch(color: hdr, viewing: session.viewing) } }.frame(width: 54, height: 48)
    }
    private var rows: some View {
        Grid(alignment: .trailing, horizontalSpacing: 8, verticalSpacing: 8) {
            ForEach(0..<3, id: \.self) { row in
                let shown = view["rows"][row]
                GridRow {
                    HStack(spacing: 6) {
                        formatMenu(row, shown: shown)
                        if !shown["space"].isNull {
                            Text(shown["space"].string).font(.caption).lineLimit(1).fixedSize().padding(.horizontal, 5)
                                .background(palette["text"].opacity(0.08), in: SquircleShape.control)
                        }
                    }.gridColumnAlignment(.leading)
                    ForEach(0..<3, id: \.self) { index in
                        ColorValueField(name: "\(row)-\(index)", shown: shown["values"][index], width: 64,
                            scrub: ["kind": "value", "row": row, "index": index], draft: draft("\(row)-\(index)"), focus: $focused, session: session)
                    }
                    copyButton("\(row)", text: shown["copy"].string)
                }
            }
            if !view["intensity"].isNull {
                GridRow {
                    Text(copy["intensity_ev"].string).gridColumnAlignment(.leading)
                    ColorValueField(name: "ev", shown: view["intensity"], width: 80, scrub: ["kind": "intensity"],
                        draft: draft("ev"), focus: $focused, session: session).gridCellColumns(3)
                    Color.clear.frame(width: 1, height: 1)
                }
            }
        }
    }
    private func formatMenu(_ row: Int, shown: JSON) -> some View {
        let forms = shown["forms"].array, current = forms.first { $0["form"].string == shown["form"].string }?["label"].string ?? ""
        return EditorMenuButton(menu: { [forms, shown] in
            AppleContextMenu(JSON(["sections": [forms.map { form -> [String: Any] in
                ["label": form["label"].raw, "enabled": true, "selected": form["form"].string == shown["form"].string,
                 "identifier": "color-format-\(row)-\(form["form"].string)", "action": ["form": form["form"].raw]]
            }]])) { session.act(["op": "form", "row": row, "form": $0["form"].raw]) }
        }, identifier: "color-format-menu-\(row)") {
            HStack(spacing: 2) {
                ZStack(alignment: .leading) {
                    ForEach(forms.indices, id: \.self) { Text(forms[$0]["label"].string).hidden() }
                    Text(current)
                }
                SharedIcon(name: "chevron-down", size: 12)
            }.fontWeight(.medium).fixedSize().padding(.horizontal, 6).frame(height: 28).contentShape(Rectangle())
        }.buttonStyle(.plain).help(copy["format"].string).accessibilityLabel(shown["label"].string)
            .accessibilityValue(current).accessibilityIdentifier("color-format-\(row)")
    }
    private func copyButton(_ name: String, text: String) -> some View {
        Button {
            ColorEditor.copy(text); copied = name
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { if copied == name { copied = nil } }
        } label: { SharedIcon(name: copied == name ? "check" : "copy").frame(width: 28, height: 28).contentShape(Rectangle()) }
            .buttonStyle(EditorControlButtonStyle()).help(copied == name ? copy["copied"].string : copy["copy"].string)
            .accessibilityLabel(copied == name ? copy["copied"].string : copy["copy"].string).accessibilityIdentifier("color-copy-\(name)")
    }
    @ViewBuilder private var status: some View {
        Text(session.error ?? " ").font(.caption).foregroundStyle(.red).opacity(session.error == nil ? 0 : 1)
            .frame(maxWidth: .infinity, alignment: .leading).accessibilityIdentifier("color-editor-error")
    }
    private var footer: some View {
        HStack(spacing: 6) {
            if sheetOpen {
                HStack(spacing: 6) {
                    HStack(spacing: 0) {
                        ColorSwatch(rgba: view["current"]["rgba"]).frame(width: 22, height: 22)
                        ColorSwatch(rgba: view["new"]["rgba"]).frame(width: 22, height: 22)
                    }.clipShape(SquircleShape.control)
                    Text(view["hex"].string).monospacedDigit()
                }
            } else {
                HStack(spacing: 4) { ForEach(Array(session.recent.prefix(8).enumerated()), id: \.offset) { tile($0.element, kind: "recent") } }
            }
            Spacer(minLength: 8)
            Button { setSheet(!sheetOpen) } label: {
                SharedIcon(name: "chevron-down").rotationEffect(.degrees(sheetOpen ? 0 : 180)).frame(width: 32, height: 32).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).help(sheetOpen ? copy["close_swatches"].string : copy["all_swatches"].string)
                .accessibilityLabel(sheetOpen ? copy["close_swatches"].string : copy["all_swatches"].string)
                .accessibilityIdentifier("color-swatches")
            Button(commonCopy["cancel"].string) { session.close(apply: false) }.buttonStyle(.bordered)
                .keyboardShortcut(sheetOpen ? nil : .cancelAction).accessibilityIdentifier("color-cancel")
            Button(copy["use_color"].string) { commitFocused(); session.close(apply: true) }
                .buttonStyle(.borderedProminent).tint(palette.accent)
                .disabled(session.blocked).keyboardShortcut(.defaultAction).accessibilityIdentifier("color-use")
        }
    }
    private var swatchSheet: some View {
        let sheet = session.sheet
        return VStack(spacing: 10) {
            HStack(spacing: 6) {
                TextField(copy["swatch_search"].string, text: $search).textFieldStyle(.roundedBorder).autocorrectionDisabled()
                    .onChange(of: search) { _, text in session.search(text) }
                    .accessibilityIdentifier("color-sheet-search")
                Button { setSheet(false) } label: { SharedIcon(name: "chevron-down").frame(width: 28, height: 28).contentShape(Rectangle()) }
                    .buttonStyle(EditorControlButtonStyle()).keyboardShortcut(.cancelAction).help(copy["close_swatches"].string)
                    .accessibilityLabel(copy["close_swatches"].string).accessibilityIdentifier("color-sheet-close")
            }
            EditorScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(sheet["sections"].array.indices, id: \.self) { index in
                        let section = sheet["sections"][index]
                        VStack(alignment: .leading, spacing: 6) {
                            HStack { Text(section["title"].string).fontWeight(.semibold); Text(section["count"].string).foregroundStyle(.secondary) }
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 32, maximum: 32), spacing: 4)], alignment: .leading, spacing: 4) {
                                ForEach(Array(section["tiles"].array.enumerated()), id: \.offset) { tile($0.element, kind: "sheet") }
                                if section["can_add"].bool {
                                    Button { session.addToPalette(section["palette"]) } label: {
                                        SharedIcon(name: "plus").frame(width: 32, height: 32).contentShape(Rectangle())
                                    }.buttonStyle(EditorControlButtonStyle()).help(nativeCopy["palettes"]["add_current"].string)
                                        .accessibilityLabel(nativeCopy["palettes"]["add_current"].string)
                                        .accessibilityIdentifier("color-sheet-add-\(index)")
                                }
                            }
                        }
                    }
                    if !sheet["empty"].isNull { Text(sheet["empty"].string).foregroundStyle(.secondary) }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
        }.padding(16).frame(maxWidth: .infinity, maxHeight: .infinity).background(palette["panel"])
            .accessibilityElement(children: .contain).accessibilityIdentifier("color-sheet")
    }
    private func tile(_ entry: JSON, kind: String) -> some View {
        Button { session.act(["op": "color", "color": entry["color"].raw]) } label: {
            PaletteFace(rgba: entry["rgba"], color: entry["color"], hdr: false, viewing: JSON()).frame(width: 28, height: 28).padding(2)
                .overlay { if entry["current"].bool { SquircleShape.control.strokeBorder(palette.accent, lineWidth: 2) } }
                .contentShape(Rectangle())
        }.buttonStyle(.plain).help(entry["detail"].string).accessibilityLabel(entry["detail"].string)
            .accessibilityIdentifier("color-tile-\(kind)")
    }
    private func setSheet(_ open: Bool) {
        if open { search = session.search; session.refreshSwatches(sheetOpen: true) }
        withAnimation(.easeOut(duration: 0.22)) { sheetOpen = open }
    }
    private func draft(_ name: String) -> Binding<String?> { Binding(get: { drafts[name] }, set: { drafts[name] = $0 }) }
    private func action(_ name: String, text: String) -> [String: Any] {
        switch name {
        case "hex": return ["op": "text", "text": text]
        case "ev": return ["op": "intensity", "text": text]
        default:
            let parts = name.split(separator: "-").compactMap { Int($0) }
            return ["op": "value", "row": parts[0], "index": parts[1], "text": text]
        }
    }
    private func commit(_ name: String) {
        guard let text = drafts[name] else { return }
        if session.act(action(name, text: text), target: name) { drafts[name] = nil }
    }
    private func commitFocused() { if let focused { commit(focused) } }
    static func copy(_ text: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
        #else
        UIPasteboard.general.string = text
        #endif
    }
}

#if os(iOS)
private struct ColorClipboardKeys: UIViewRepresentable {
    let editing: Bool
    let copy: () -> Void
    let paste: () -> Void
    func makeUIView(context: Context) -> CaptureView { CaptureView() }
    func updateUIView(_ view: CaptureView, context: Context) {
        view.editing = editing; view.copyColor = copy; view.pasteColor = paste
        if !editing { DispatchQueue.main.async { view.claimFocus() } }
    }
    static func dismantleUIView(_ view: CaptureView, coordinator: ()) { view.copyColor = nil; view.pasteColor = nil; if view.isFirstResponder { view.resignFirstResponder() } }
    final class CaptureView: UIView {
        var editing = false
        var copyColor: (() -> Void)?
        var pasteColor: (() -> Void)?
        override var canBecomeFirstResponder: Bool { true }
        override func didMoveToWindow() { super.didMoveToWindow(); DispatchQueue.main.async { self.claimFocus() } }
        private func firstResponder(in view: UIView) -> UIView? {
            view.isFirstResponder ? view : view.subviews.lazy.compactMap { self.firstResponder(in: $0) }.first
        }
        func claimFocus() {
            guard let window, window.isKeyWindow, !editing, !isFirstResponder, copyColor != nil, !NativeTextContext.composing else { return }
            let responder = firstResponder(in: window)
            guard !(responder is UITextInput) else { return }
            becomeFirstResponder()
        }
        override var keyCommands: [UIKeyCommand]? {
            [UIKeyModifierFlags.command, .control].flatMap { modifier in
                [UIKeyCommand(input: "c", modifierFlags: modifier, action: #selector(copyValue)),
                 UIKeyCommand(input: "x", modifierFlags: modifier, action: #selector(cutValue)),
                 UIKeyCommand(input: "v", modifierFlags: modifier, action: #selector(pasteValue))]
            }
        }
        @objc private func copyValue() { if isFirstResponder && !NativeTextContext.composing { copyColor?() } }
        @objc private func cutValue() {}
        @objc private func pasteValue() { if isFirstResponder && !NativeTextContext.composing { pasteColor?() } }
    }
}
#endif

private struct ColorEditorLayout: Layout {
    private let columns: CGFloat = 24, rows: CGFloat = 16, below: CGFloat = 18
    private func sizes(_ subviews: Subviews) -> [CGSize] { subviews.map { $0.sizeThatFits(.unspecified) } }
    private func wide(_ sizes: [CGSize], width: CGFloat?) -> Bool {
        sizes.count == 3 && (width ?? .infinity) >= sizes[1].width + columns + max(sizes[0].width, sizes[2].width)
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = sizes(subviews)
        guard sizes.count == 3 else { return .zero }
        if wide(sizes, width: proposal.width) {
            return CGSize(width: sizes[1].width + columns + max(sizes[0].width, sizes[2].width),
                height: max(sizes[1].height, sizes[0].height + below + sizes[2].height))
        }
        return CGSize(width: sizes.map(\.width).max() ?? 0, height: sizes[0].height + sizes[1].height + sizes[2].height + rows * 2)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let sizes = sizes(subviews)
        guard sizes.count == 3 else { return }
        if wide(sizes, width: bounds.width) {
            let left = bounds.minX + (bounds.width - sizes[1].width - columns - max(sizes[0].width, sizes[2].width)) / 2
            let right = left + sizes[1].width + columns
            subviews[1].place(at: CGPoint(x: left, y: bounds.minY), proposal: ProposedViewSize(sizes[1]))
            subviews[0].place(at: CGPoint(x: right, y: bounds.minY), proposal: ProposedViewSize(sizes[0]))
            subviews[2].place(at: CGPoint(x: right, y: bounds.minY + sizes[0].height + below), proposal: ProposedViewSize(sizes[2]))
        } else {
            var y = bounds.minY
            for index in [0, 1, 2] {
                subviews[index].place(at: CGPoint(x: bounds.midX - sizes[index].width / 2, y: y), proposal: ProposedViewSize(sizes[index]))
                y += sizes[index].height + rows
            }
        }
    }
}

private struct ColorValueField: View {
    let name: String
    let shown: JSON
    let width: CGFloat
    var large = false
    var scrub: [String: Any]?
    @Binding var draft: String?
    let focus: FocusState<String?>.Binding
    @ObservedObject var session: ColorEditorSession
    @State private var opened = false
    @State private var scrubbing = false
    private var invalid: Bool { session.errorTarget == name }
    private var editing: Bool { opened || invalid || focus.wrappedValue == name }
    var body: some View {
        Group {
            if editing {
                TextField(shown["name"].string, text: Binding(get: { draft ?? shown["edit"].string }, set: { draft = $0 }))
                    .textFieldStyle(.plain).multilineTextAlignment(large ? .leading : .trailing).monospacedDigit().autocorrectionDisabled()
                    .focused(focus, equals: name)
                    .onAppear { DispatchQueue.main.async { focus.wrappedValue = name } }
                    .onChange(of: focus.wrappedValue) { _, now in if now != name { opened = false } }
                    .onSubmit { focus.wrappedValue = nil }
                    .onKeyPress(.escape) { draft = nil; session.dismissError(name); opened = false; focus.wrappedValue = nil; return .handled }
                    .onKeyPress(keys: [.upArrow, .downArrow]) { press in
                        guard scrub != nil else { return .ignored }
                        step(press.key == .upArrow ? 2 : -2); return .handled
                    }
                    .accessibilityIdentifier("color-entry-\(name)")
            } else {
                Text(shown["text"].string).monospacedDigit().lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: large ? .leading : .trailing).contentShape(Rectangle())
                    .onTapGesture { draft = nil; opened = true }
                    .gesture(DragGesture(minimumDistance: 4)
                        .onChanged { drag in
                            guard let scrub else { return }
                            scrubbing = true
                            session.act(["op": "scrub", "target": scrub, "pixels": -drag.translation.height, "speed": Self.speed], target: name)
                        }
                        .onEnded { _ in
                            guard scrubbing else { return }
                            scrubbing = false; session.act(["op": "end_scrub", "cancel": false], target: name)
                        })
                    .accessibilityElement().accessibilityLabel(shown["name"].string).accessibilityValue(shown["text"].string)
                    .accessibilityAddTraits(.isButton).accessibilityIdentifier("color-value-\(name)")
            }
        }.font(large ? .title2.weight(.medium) : .body).frame(width: width, height: large ? 34 : 28)
            .padding(.horizontal, 6)
            .background(invalid ? Color.red.opacity(0.12) : Color.primary.opacity(editing ? 0.08 : 0.04), in: SquircleShape.control)
    }
    private func step(_ pixels: Double) {
        guard let scrub else { return }
        draft = nil
        session.act(["op": "scrub", "target": scrub, "pixels": pixels, "speed": Self.speed], target: name)
        session.act(["op": "end_scrub", "cancel": false], target: name)
    }
    private static var speed: String {
        #if os(macOS)
        let flags = NSEvent.modifierFlags
        return flags.contains(.shift) ? "fast" : flags.contains(.option) || flags.contains(.control) ? "fine" : "normal"
        #else
        return "normal"
        #endif
    }
}

private struct ColorEditorWheel: View {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    @ObservedObject var session: ColorEditorSession
    let side: CGFloat
    @StateObject private var resources = ColorPanelLayoutCache()
    var body: some View {
        let panel = session.view["panel"], hdr = panel["hdr"].bool
        let spec = resources.layout(side: side, hdr: hdr), layout = spec["layout"], wheel = layout["wheel"]
        let height = hdr ? spec["height"].number : side
        let visible = hdr ? height - wheel[1].number : wheel[2].number
        ZStack(alignment: .topLeading) {
            ColorWheelDrawing(model: panel, bounds: wheel, viewing: hdr ? session.viewing : JSON())
                .frame(width: side, height: side).allowsHitTesting(false)
            ColorWheelInput(shape: ColorWheelShape(panel["shape"].string).rawValue,
                context: "\(panel["rgb_space"].string):\(panel["shape"].string)", value: panel["readout_description"].string) { part, point, size in
                session.act(["op": "wheel", "action": ["op": "pick_wheel", "part": part == 1 ? "hue" : "field", "point": [point.x, point.y], "size": size]])
            }.colorPlaced(wheel, id: "editor-wheel")
            if hdr {
                HDRIntensityArc(model: panel, viewing: session.viewing, identity: panel["rgb_space"].string, language: interfaceLanguage,
                    geometry: spec["arc"], caption: layout["intensity_caption"], size: side) { stops in
                    session.act(["op": "wheel", "action": ["op": "hdr_intensity", "stops": stops]])
                }.frame(width: side, height: height)
            }
        }.frame(width: side, height: height, alignment: .topLeading)
            .offset(x: -wheel[0].number, y: -wheel[1].number)
            .frame(width: wheel[2].number, height: visible, alignment: .topLeading).clipped()
            .accessibilityElement(children: .contain).accessibilityIdentifier("color-editor-wheel")
    }
}

private struct ColorPickStrip: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var session: ColorEditorSession
    @State private var corner = "top_right"
    @State private var size = CGSize(width: 220, height: 56)
    @State private var hover: CGPoint?
    var body: some View {
        let picker = store.state["color_picker"]
        let sample = picker["preview"].isNull ? session.view["value"] : picker["preview"]
        let shown = ColorUI.resolve(["type": "editor_strip", "editor": session.editor.raw, "sample": sample.raw], language: store.interfaceLanguage)
        let previews = ColorUI.resolve(["type": "preview", "colors": [session.view["value"].raw, sample.raw],
            "document_space": session.view["panel"]["rgb_space"].raw, "display_space": "DisplayP3", "rendition": session.rendition.raw])
        let area = store.snapshot["layout"]["work_area"].rect
        let avoid: [Any] = [picker["sample_point"], hover.map { JSON([$0.x, $0.y]) } ?? JSON()].filter { !$0.isNull }.map(\.raw)
        let placed = ColorUI.resolve(["type": "strip_placement", "area": [area.minX, area.minY, area.width, area.height],
            "size": [size.width, size.height], "scale": 1, "avoid": avoid, "corner": corner])
        Button { session.stopPicking() } label: {
            HStack(spacing: 10) {
                HStack(spacing: 0) {
                    ColorSwatch(rgba: previews[0]["rgba"]).frame(width: 26, height: 36)
                    ColorSwatch(rgba: previews[1]["rgba"]).frame(width: 26, height: 36)
                }.clipShape(SquircleShape.control)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(shown["hex"].string).fontWeight(.medium)
                        if !shown["intensity"].isNull { Text(shown["intensity"].string).foregroundStyle(.secondary) }
                    }
                    HStack(spacing: 6) {
                        Text(shown["label"].string).foregroundStyle(.secondary)
                        Text(shown["values"].array.map(\.string).joined(separator: "  "))
                    }.font(.caption)
                }.monospacedDigit()
            }.padding(.horizontal, 10).padding(.vertical, 8).fixedSize()
        }.buttonStyle(.plain)
            .background(EditorPalette(source: store.state["palette"]).glassPanel, in: SquircleShape.surface)
            .onGeometryChange(for: CGSize.self) { $0.size } action: { size = $0 }
            .onContinuousHover { phase in
                if case .active(let point) = phase { hover = CGPoint(x: point.x + placed["origin"][0].number, y: point.y + placed["origin"][1].number) }
                else { hover = nil }
            }
            .onChange(of: placed["corner"].string) { _, next in if !next.isEmpty { corner = next } }
            .help(store.catalog["native_copy"]["color"]["picking_strip"].string)
            .accessibilityLabel(store.catalog["native_copy"]["color"]["picking_strip"].string)
            .accessibilityIdentifier("color-strip")
            .offset(x: placed["origin"][0].number, y: placed["origin"][1].number)
    }
}
