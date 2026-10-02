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

struct SelectionRefinePanel: View {
    @ObservedObject var store: EditorStore
    let view: JSON
    let palette: EditorPalette
    @Environment(\.capyCommonCopy) private var common
    @State private var commitNumber: ((Bool) -> Bool)?
    private func send(_ action: [String: Any]) { store.dispatch(["type": "selection", "action": action]) }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(view["title"].string).fontWeight(.bold).accessibilityIdentifier("selection-refine-title")
            NumberControl(store: store, label: view["label"].string, value: view["radius"].number,
                control: view["numeric"], identifier: "selection-refine-value", registerAdmission: { _, admission in commitNumber = admission }) { value, completion in
                send(["op": "resize_radius", "radius": value]); completion(nil)
            }.id(view["kind"].string)
            HStack(spacing: 8) {
                Spacer()
                Button(common["cancel"].string) { _ = commitNumber?(true); send(["op": "cancel_resize"]) }.buttonStyle(.bordered)
                    .accessibilityIdentifier("selection-refine-cancel")
                Button(common["apply"].string) { if commitNumber?(false) != false { send(["op": "apply_resize"]) } }.buttonStyle(.borderedProminent).tint(palette.accent)
                    .accessibilityIdentifier("selection-refine-apply")
            }.focusable(false)
        }.padding(.top, 14).padding(.horizontal, 16).padding(.bottom, 12)
            .foregroundStyle(palette["text"])
            .background {
                SquircleShape.surface.fill(palette["panel"]).shadow(color: .black.opacity(0.27), radius: 4, y: 2)
            }
            .accessibilityElement(children: .contain).accessibilityIdentifier("selection-refine-panel")
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
