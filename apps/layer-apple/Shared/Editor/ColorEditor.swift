import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

struct ManagedColorButton: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    let label: String
    let identifier: String
    let value: JSON
    let colors: JSON
    var viewing = JSON()
    var opaque = false
    var titled = true
    var swatchWidth: CGFloat = 48
    let change: (JSON) -> Void
    @State private var editing = false
    var body: some View {
        let preview = ColorUI.preview(value)
        HStack {
            if titled { Text(label).frame(maxWidth: .infinity, alignment: .leading) }
            Button { editing = true } label: {
                ColorSwatch(rgba: preview["rgba"], shape: .control).frame(width: swatchWidth, height: 28)
                    .overlay(SquircleShape.control.stroke(.primary.opacity(0.3), lineWidth: 1))
            }.buttonStyle(.plain).accessibilityLabel(label).accessibilityIdentifier(identifier + "-color")
                .help(preview["in_gamut"].bool ? label : nativeCopy["color"]["outside_p3"].string)
                .sheet(isPresented: $editing) {
                    ColorEditor(colors: colors, value: value, opaque: opaque, viewing: viewing) { color, _ in change(color); editing = false }
                }
        }
    }
}

/// Shared Rust owns the draft, parsing, conversion and formats; only Use Color publishes.
struct ColorEditor: View {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.capyCommonCopy) private var commonCopy
    @Environment(\.dismiss) private var dismiss
    @State private var editor: JSON
    @State private var view: JSON
    @State private var drafts: [String: String] = [:]
    @State private var error: String?
    @State private var refused: [String: Any]?
    @FocusState private var focused: String?
    let viewing: JSON
    let use: (JSON, Double?) -> Void
    init(colors: JSON, slot: String? = nil, value: JSON? = nil, opaque: Bool = false, viewing: JSON = JSON(), use: @escaping (JSON, Double?) -> Void) {
        var request: [String: Any] = ["type": "editor_open", "opaque": opaque, "display_space": "DisplayP3", "rendition": viewing["recipe"].raw]
        if !colors.isNull { request["colors"] = colors.raw }
        if let slot { request["slot"] = slot } else if let value { request["color"] = value.raw }
        let opened = ColorUI.resolve(request)
        _editor = State(initialValue: opened["editor"]); _view = State(initialValue: opened["view"])
        _error = State(initialValue: opened["editor"].isNull ? opened["error"].string : nil)
        self.viewing = viewing; self.use = use
    }
    private func send(_ action: [String: Any]?) -> String? {
        var request: [String: Any] = ["type": "editor", "editor": editor.raw, "display_space": "DisplayP3", "rendition": viewing["recipe"].raw]
        if let action { request["action"] = action }
        let next = ColorUI.resolve(request, language: interfaceLanguage)
        if next["editor"].isNull { return next["error"].string }
        editor = next["editor"]; view = next["view"]
        return next["error"].isNull ? nil : next["error"].string
    }
    private func act(_ action: [String: Any], field: String? = nil) {
        let failure = send(action)
        error = failure; refused = failure == nil ? nil : action
        if failure == nil, let field { drafts[field] = nil }
    }
    private func copy(_ text: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
        #else
        UIPasteboard.general.string = text
        #endif
    }
    private func field(_ name: String, shown: JSON, width: CGFloat, action: @escaping (String) -> [String: Any]) -> some View {
        TextField(shown["name"].string.isEmpty ? name : shown["name"].string, text: Binding(
            get: { drafts[name] ?? (focused == name ? shown["edit"].string : shown["text"].string) },
            set: { drafts[name] = $0 }))
            .textFieldStyle(.plain).multilineTextAlignment(.trailing).monospacedDigit().autocorrectionDisabled()
            .frame(width: width).focused($focused, equals: name)
            .onSubmit { if let text = drafts[name] { act(action(text), field: name) } }
            .accessibilityIdentifier("color-value-\(name)")
    }
    var body: some View {
        let color = nativeCopy["color"]
        VStack(alignment: .leading, spacing: 14) {
            Text(color["edit"].string).font(.headline)
            HStack(alignment: .top, spacing: 12) {
                VStack(spacing: 2) {
                    HStack(spacing: 0) {
                        Group {
                            if viewing["hdr"].bool { HDRColorSwatch(color: view["current_value"], viewing: viewing) } else { ColorSwatch(rgba: view["current"]["rgba"]) }
                        }.frame(width: 54, height: 48).contentShape(Rectangle()).onTapGesture { act(["op": "revert"]) }
                            .accessibilityLabel(color["current"].string).accessibilityAddTraits(.isButton).accessibilityIdentifier("color-current")
                        Group {
                            if viewing["hdr"].bool { HDRColorSwatch(color: view["value"], viewing: viewing) } else { ColorSwatch(rgba: view["new"]["rgba"]) }
                        }.frame(width: 54, height: 48).accessibilityLabel(color["new"].string).accessibilityIdentifier("color-new")
                    }.clipShape(SquircleShape.control)
                    HStack(spacing: 0) {
                        Text(color["current"].string).frame(maxWidth: .infinity)
                        Text(color["new"].string).frame(maxWidth: .infinity)
                    }.font(.caption).foregroundStyle(.secondary).frame(width: 108)
                }
                Spacer()
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(color["hex"].string).font(.caption.bold()).foregroundStyle(.secondary)
                        if !view["hex_note"].isNull { Text(view["hex_note"]["text"].string).font(.caption).help(view["hex_note"]["tip"].string) }
                    }
                    HStack(spacing: 4) {
                        field("hex", shown: JSON(["text": view["hex"].raw, "edit": view["hex"].raw, "name": color["hex"].raw]), width: 108) { ["op": "text", "text": $0] }
                            .font(.title2.weight(.semibold)).multilineTextAlignment(.leading)
                        Button { copy(view["hex"].string) } label: { SharedIcon(name: "copy") }.buttonStyle(.plain)
                            .help(color["copy"].string).accessibilityLabel(color["copy"].string).accessibilityIdentifier("color-copy-hex")
                    }
                }
            }
            Grid(alignment: .trailing, horizontalSpacing: 8, verticalSpacing: 6) {
                ForEach(0..<3, id: \.self) { row in
                    let shown = view["rows"][row]
                    GridRow {
                        HStack(spacing: 4) {
                            Menu(shown["label"].string) {
                                ForEach(shown["forms"].array.indices, id: \.self) { index in
                                    let form = shown["forms"].array[index]
                                    Button(form["label"].string) { act(["op": "form", "row": row, "form": form["form"].string]) }
                                }
                            }.menuStyle(.borderlessButton).fixedSize().accessibilityIdentifier("color-format-\(row)")
                            if !shown["space"].isNull { Text(shown["space"].string).font(.caption).padding(.horizontal, 5).background(.primary.opacity(0.08), in: SquircleShape.control) }
                        }.gridColumnAlignment(.leading)
                        ForEach(0..<3, id: \.self) { index in
                            field("\(row)-\(index)", shown: shown["values"][index], width: 64) { ["op": "value", "row": row, "index": index, "text": $0] }
                        }
                        Button { copy(shown["copy"].string) } label: { SharedIcon(name: "copy") }.buttonStyle(.plain)
                            .help(color["copy"].string).accessibilityLabel(color["copy"].string).accessibilityIdentifier("color-copy-\(row)")
                    }
                }
                if !view["intensity"].isNull {
                    GridRow {
                        Text(color["intensity_ev"].string).gridColumnAlignment(.leading)
                        field("ev", shown: view["intensity"], width: 80) { ["op": "intensity", "text": $0] }.gridCellColumns(3)
                        Color.clear.frame(width: 1, height: 1)
                    }
                }
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red).accessibilityIdentifier("color-editor-error") }
            HStack {
                Button(commonCopy["cancel"].string) { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(color["use_color"].string) { use(view["value"], view["stops"].isNull ? nil : view["stops"].number) }
                    .disabled(view["value"].isNull || refused != nil)
                    .keyboardShortcut(.defaultAction).accessibilityIdentifier("color-use")
            }
        }.onChange(of: interfaceLanguage) { _, _ in _ = send(nil); if let refused { error = send(refused) } }
        .onChange(of: focused) { previous, _ in
            if let previous, let text = drafts[previous] {
                let action: [String: Any]
                switch previous {
                case "hex": action = ["op": "text", "text": text]
                case "ev": action = ["op": "intensity", "text": text]
                default:
                    let parts = previous.split(separator: "-").compactMap { Int($0) }
                    action = ["op": "value", "row": parts[0], "index": parts[1], "text": text]
                }
                act(action, field: previous)
            }
        }
        .padding(20).frame(minWidth: 360, idealWidth: 460, maxWidth: 560)
    }
}
