import SwiftUI
#if os(macOS)
import AppKit
#else
import GameController
#endif

enum SelectionModes {
    static let commands: Set<String> = ["selection_new", "selection_add", "selection_subtract", "selection_intersect"]
}

struct SelectionModeGroup: View {
    @ObservedObject var store: EditorStore
    let actions: [JSON]
    var height: CGFloat = 44
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        HStack(spacing: 0) {
            ForEach(actions.indices, id: \.self) { index in
                let command = store.command(actions[index]["command"].string)
                if index > 0 { Rectangle().fill(palette["text"].opacity(0.2)).frame(width: 1) }
                Button { store.invoke(command["id"].string) } label: {
                    SharedIcon(name: command["icon"].string, size: 20)
                        .frame(maxWidth: .infinity, minHeight: height).contentShape(Rectangle())
                }.buttonStyle(SelectionSegmentStyle(selected: command["selected"].bool,
                    shape: SquircleShape.control.segment(index, of: actions.count)))
                    .disabled(!command["enabled"].bool).opacity(command["enabled"].bool ? 1 : 0.36)
                    .help(command["tooltip"].string)
                    .accessibilityLabel(command["label"].string)
                    .accessibilityAddTraits(command["selected"].bool ? .isSelected : [])
                    .accessibilityIdentifier("tool-action-" + command["id"].string)
            }
        }.fixedSize(horizontal: false, vertical: true)
            .overlay { SquircleShape.control.strokeBorder(palette["text"].opacity(0.2), lineWidth: 1).allowsHitTesting(false) }
            .accessibilityElement(children: .contain).accessibilityLabel("Selection mode")
    }
}

private struct SelectionSegmentStyle: ButtonStyle {
    let selected: Bool
    let shape: SquircleShape
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, selected: selected, shape: shape) }
    private struct Face: View {
        let configuration: Configuration
        let selected: Bool
        let shape: SquircleShape
        @Environment(\.editorPalette) private var palette
        var body: some View {
            configuration.label.background {
                if selected { shape.fill(palette.active) }
                else if configuration.isPressed { shape.fill(.foreground).opacity(0.16) }
            }
        }
    }
}

struct SelectionMenuButton: View {
    @ObservedObject var store: EditorStore
    let label: String
    let kind: String
    @State private var menu = JSON()
    var body: some View {
        Button {
            store.query(["type": "selection_menu", "kind": kind]) { menu = $0 }
        } label: {
            HStack(spacing: 6) {
                Text(label).fontWeight(.bold).lineLimit(1)
                SharedIcon(name: "chevron-down", size: 12)
            }.frame(maxWidth: .infinity, minHeight: 44).padding(.horizontal, 12).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle())
            .help(label).accessibilityLabel(label)
            .accessibilityIdentifier("selection-menu-" + kind)
            .editorPopover(isPresented: Binding(get: { !menu.isNull }, set: { if !$0 { menu = JSON() } })) {
                EditorActionMenu(model: AppleContextMenu(menu) { store.dispatch($0) },
                    identifier: "selection-menu", dismiss: { menu = JSON() })
            }
    }
}

struct SelectionResizeDialog: ViewModifier {
    @ObservedObject var store: EditorStore
    private var view: JSON { store.state["layer_tools"]["selection_resize"] }
    private func send(_ action: [String: Any]) { store.dispatch(["type": "selection", "action": action]) }
    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(get: { !view.isNull }, set: { if !$0 && !view.isNull { send(["op": "cancel_resize"]) } })) {
            VStack(alignment: .leading, spacing: 16) {
                Text(view["title"].string).font(.headline)
                NumberControl(store: store, label: "Distance", value: view["radius"].number,
                    control: view["numeric"], identifier: "selection-resize-distance") { value, completion in
                    send(["op": "resize_radius", "radius": value]); completion(nil)
                }
                HStack {
                    Spacer()
                    Button("Cancel", role: .cancel) { send(["op": "cancel_resize"]) }
                        .keyboardShortcut(.cancelAction).accessibilityIdentifier("selection-resize-cancel")
                    Button("Apply") { send(["op": "apply_resize"]) }
                        .keyboardShortcut(.defaultAction).accessibilityIdentifier("selection-resize-apply")
                }
            }.padding(24).frame(minWidth: 320, idealWidth: 360, maxWidth: 420)
                .buttonStyle(.bordered).presentationSizing(.fitted)
                .interactiveDismissDisabled()
                .modifier(EditorPopupPresentation())
        }
    }
}

enum ThumbnailSelectionLoad {
    struct Modifiers { let shift: Bool; let alt: Bool }
    @MainActor static func current() -> Modifiers? {
        #if os(macOS)
        let flags = NSEvent.modifierFlags
        guard flags.contains(.command) else { return nil }
        return Modifiers(shift: flags.contains(.shift), alt: flags.contains(.option))
        #else
        guard let keys = GCKeyboard.coalesced?.keyboardInput else { return nil }
        func held(_ codes: GCKeyCode...) -> Bool { codes.contains { keys.button(forKeyCode: $0)?.isPressed == true } }
        guard held(.leftGUI, .rightGUI) else { return nil }
        return Modifiers(shift: held(.leftShift, .rightShift), alt: held(.leftAlt, .rightAlt))
        #endif
    }
}
