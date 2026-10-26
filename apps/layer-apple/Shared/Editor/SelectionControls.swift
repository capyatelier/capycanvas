import SwiftUI
#if os(macOS)
import AppKit
#else
import GameController
#endif

struct SelectionModeGroup: View {
    @ObservedObject var store: EditorStore
    let actions: [JSON]
    var height: CGFloat = 44
    var iconSize: CGFloat = 20
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        HStack(spacing: 0) {
            ForEach(actions.indices, id: \.self) { index in
                let command = store.command(actions[index]["command"].string)
                if index > 0 { Rectangle().fill(palette["text"].opacity(0.2)).frame(width: 1) }
                Button { store.invoke(command["id"].string) } label: {
                    SharedIcon(name: command["icon"].string, size: iconSize)
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
            .accessibilityElement(children: .contain).accessibilityLabel(store.catalog["tool_action_groups"].array.first { $0["id"].string == actions.first?["group"].string }?["label"].string ?? "")
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
    var compact = false
    @State private var menu = JSON()
    var body: some View {
        Button {
            store.query(["type": "selection_menu", "kind": kind]) { menu = $0 }
        } label: {
            HStack(spacing: 6) {
                Text(label).fontWeight(.bold).lineLimit(1)
                SharedIcon(name: "chevron-down", size: 12)
            }.frame(maxWidth: .infinity, minHeight: compact ? store.panelMetrics.height : 44).padding(.horizontal, compact ? store.panelMetrics.padding : 12).contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle())
            .help(label).accessibilityLabel(label)
            .accessibilityIdentifier("selection-menu-" + kind)
            .editorPopover(isPresented: Binding(get: { !menu.isNull }, set: { if !$0 { menu = JSON() } })) {
                EditorActionMenu(model: AppleContextMenu(menu) { store.dispatch($0) },
                    identifier: "selection-menu", dismiss: { menu = JSON() })
            }
    }
}

struct CanvasPreviewPanel: View {
    struct Operations {
        let type: String, value: String, apply: String, cancel: String, identifier: String
        static let refine = Operations(type: "selection", value: "resize_radius", apply: "apply_resize", cancel: "cancel_resize", identifier: "selection-refine")
        static let frequencySeparation = Operations(type: "frequency_separation", value: "radius", apply: "apply", cancel: "cancel", identifier: "frequency-separation")
    }
    @ObservedObject var store: EditorStore
    let view: JSON
    let operations: Operations
    let palette: EditorPalette
    @Environment(\.capyCommonCopy) private var common
    @State private var admissions = NumericAdmissions()
    @State private var responded = false
    private func send(_ op: String, _ fields: [String: Any] = [:]) {
        store.dispatch(["type": operations.type, "action": fields.merging(["op": op]) { $1 }])
    }
    private func respond(_ apply: Bool) {
        guard !responded else { return }
        if apply {
            guard admissions.finish() else { return }
            responded = true; send(operations.apply)
        } else {
            responded = true; _ = admissions.finish(true); send(operations.cancel)
        }
    }
    var body: some View {
        let identifier = operations.identifier
        VStack(alignment: .leading, spacing: 10) {
            Text(view["title"].string).fontWeight(.bold).accessibilityIdentifier(identifier + "-title")
            NumberControl(store: store, label: view["label"].string, value: view["radius"].number,
                control: view["numeric"], identifier: identifier + "-value", registerAdmission: { key, change in admissions.receive(key, change) }) { value, completion in
                send(operations.value, ["radius": value]); completion(nil)
            }.id(view["kind"].string)
            HStack(spacing: 8) {
                Spacer()
                Button(common["cancel"].string) { respond(false) }.buttonStyle(.bordered)
                    .keyboardShortcut(.cancelAction).accessibilityIdentifier(identifier + "-cancel")
                Button(common["apply"].string) { respond(true) }.buttonStyle(.borderedProminent).tint(palette.accent)
                    .keyboardShortcut(.defaultAction).accessibilityIdentifier(identifier + "-apply")
            }.focusable(false)
            #if os(macOS)
            .background(PreviewPanelKeys(workspace: store.workspace, respond: respond).frame(width: 0, height: 0))
            #endif
        }.padding(.top, 14).padding(.horizontal, 16).padding(.bottom, 12)
            .foregroundStyle(palette["text"])
            .background {
                SquircleShape.surface.fill(palette["panel"]).shadow(color: .black.opacity(0.27), radius: 4, y: 2)
            }
            .accessibilityElement(children: .contain).accessibilityIdentifier(identifier + "-panel")
    }
}

#if os(macOS)
private struct PreviewPanelKeys: NSViewRepresentable {
    let workspace: WorkspacePresentation
    let respond: (Bool) -> Void
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView {
        let view = NSView(), coordinator = context.coordinator
        coordinator.respond = respond
        coordinator.monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak view, weak coordinator, weak workspace] event in
            guard let window = view?.window, event.window === window, window.isKeyWindow, !event.isARepeat,
                  !(window.firstResponder is NSText), workspace?.hasPopover(excluding: UUID()) == false,
                  event.modifierFlags.intersection([.command, .control, .option, .shift]).isEmpty,
                  let apply = [36: true, 76: true, 53: false][event.keyCode] else { return event }
            coordinator?.respond?(apply)
            return nil
        }
        return view
    }
    func updateNSView(_ view: NSView, context: Context) { context.coordinator.respond = respond }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor) }
        coordinator.monitor = nil; coordinator.respond = nil
    }
    final class Coordinator {
        var monitor: Any?
        var respond: ((Bool) -> Void)?
    }
}
#endif

enum ThumbnailSelectionLoad {
    struct Modifiers { let shift: Bool; let alt: Bool; let toggle: Bool }
    @MainActor static func current() -> Modifiers? {
        let keys = modifiers()
        return keys.toggle ? keys : nil
    }
    @MainActor static func modifiers() -> Modifiers {
        #if os(macOS)
        let flags = NSEvent.modifierFlags
        return Modifiers(shift: flags.contains(.shift), alt: flags.contains(.option), toggle: flags.contains(.command))
        #else
        guard let keys = GCKeyboard.coalesced?.keyboardInput else { return Modifiers(shift: false, alt: false, toggle: false) }
        func held(_ codes: GCKeyCode...) -> Bool { codes.contains { keys.button(forKeyCode: $0)?.isPressed == true } }
        return Modifiers(shift: held(.leftShift, .rightShift), alt: held(.leftAlt, .rightAlt), toggle: held(.leftGUI, .rightGUI))
        #endif
    }
}
