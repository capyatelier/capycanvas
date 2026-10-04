import SwiftUI

/// Stable shared workspace identities; selecting a segment uses ordinary save,
/// switch and window-ownership policy rather than reapplying a shipped preset.
struct WorkspaceSwitcher: View {
    @ObservedObject var workspaces: WorkspaceController
    let palette: EditorPalette
    var textSize: Double = 44.0 / 3
    var maximumWidth: CGFloat = 420
    var tile: CGFloat = 36
    private var choices: [JSON] { workspaces.view["switcher_display"].array }
    private var naturalWidth: CGFloat {
        Self.naturalWidth(choices, textSize: textSize)
    }
    static func naturalWidth(_ choices: [JSON], textSize: Double) -> CGFloat {
        10 + CGFloat(max(0, choices.count - 1)) * 2 + choices.reduce(0) { width, workspace in
            width + min(110, EditorTextMetrics.width(workspace["title"].string, size: textSize, weight: .medium))
                + 16
        } + 2 + WorkspaceSwitcherOptions.size.width
    }
    var body: some View {
        Group {
            if maximumWidth + 0.5 < naturalWidth {
                EditorMenuButton(menu: { Self.menu(workspaces) }, identifier: "workspace-switcher-menu") {
                    HStack(spacing: 8) {
                        Text(choices.first(where: { $0["current"].bool })?["title"].string ?? workspaces.view["switcher_menu"]["title"].string)
                            .lineLimit(1)
                        Image(systemName: "chevron.down").font(.system(size: 11))
                    }.padding(.horizontal, 12).frame(width: maximumWidth, height: tile)
                }.buttonStyle(HeaderButtonStyle(radius: tile / 2))
                    .disabled(!workspaces.ready || workspaces.readOnly)
                    .modifier(WorkspaceSwitcherContext(workspaces: workspaces))
            } else {
                segments.glassSurface(SquircleShape.tile, fill: palette.glassSwitcher)
            }
        }
        .accessibilityElement(children: .contain).accessibilityLabel(workspaces.view["switcher_menu"]["title"].string)
        .accessibilityIdentifier("workspace-switcher")
        .modifier(HeaderControlMeasurement(id: "workspace-switcher"))
    }
    @MainActor static func menu(_ workspaces: WorkspaceController) -> AppleContextMenu {
        AppleContextMenu(workspaces.view["switcher_menu"].replacing("title", with: JSON(""))) { workspaces.store?.dispatch($0) }
    }
    @MainActor static func options(_ workspaces: WorkspaceController) -> AppleContextMenu {
        AppleContextMenu(workspaces.view["switcher_options"].replacing("title", with: JSON(""))) { workspaces.store?.dispatch($0) }
    }
    private var choicesEnabled: Bool { workspaces.ready && !workspaces.busy && !workspaces.readOnly && !workspaces.presented }
    private var segments: some View {
        WorkspaceNameWidth(natural: naturalWidth, maximum: maximumWidth) {
            HStack(spacing: 2) {
                ScrollViewReader { scroll in
                    EditorScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 2) {
                            ForEach(choices, id: \.switcherID) { workspace in
                                choice(workspace).id(workspace["id"].string)
                            }
                        }.padding([.leading, .vertical], 5)
                    }.frame(height: 36)
                        .onChange(of: choices.first?["id"].string) { _, first in
                            if let first, first == workspaces.view["id"].string { scroll.scrollTo(first, anchor: .leading) }
                        }
                }.disabled(!choicesEnabled).modifier(WorkspaceSwitcherContext(workspaces: workspaces))
                WorkspaceSwitcherOptions(workspaces: workspaces, palette: palette)
            }.padding(.trailing, 5)
        }
    }
    private func choice(_ workspace: JSON) -> some View {
        let selected = workspace["current"].bool
        return Button { workspaces.switchTo(workspace["id"].string) } label: {
            WorkspaceNameWidth(natural: EditorTextMetrics.width(workspace["title"].string, size: textSize, weight: .medium),
                maximum: 110) {
                Text(workspace["title"].string).font(EditorTextMetrics.font(size: textSize, weight: .medium)).lineLimit(1)
            }.padding(.horizontal, 8).frame(height: 26)
                .foregroundStyle(palette["text"])
        }.buttonStyle(SwitcherChoiceStyle(selected: selected, palette: palette)).fixedSize(horizontal: true, vertical: false)
            .help("Switch to \(workspace["title"].string) workspace")
            .accessibilityIdentifier("workspace-switch-" + workspace["id"].string)
            .accessibilityAddTraits(selected ? .isSelected : [])
            .modifier(HeaderControlMeasurement(id: "workspace-switch-" + workspace["id"].string))
    }
}

private struct WorkspaceSwitcherContext: ViewModifier {
    @ObservedObject var workspaces: WorkspaceController
    @State private var popupID = UUID()
    func body(content: Content) -> some View {
        content.nativeEditorContextMenu(identity: "workspace-switcher", load: { $0(WorkspaceSwitcher.options(workspaces)) },
            visibility: { workspaces.store?.workspace.popover(popupID, open: $0) })
    }
}

struct WorkspaceSwitcherOptions: View {
    static let size = CGSize(width: 20, height: 26)
    @ObservedObject var workspaces: WorkspaceController
    let palette: EditorPalette
    @State private var open = false
    @State private var popupID = UUID()
    var body: some View {
        let label = workspaces.view["switcher_options_label"].string
        Button { open.toggle() } label: {
            SharedIcon(name: "more-small", size: 16).foregroundStyle(palette["text"].opacity(0.7))
                .frame(width: Self.size.width, height: Self.size.height).contentShape(Rectangle())
        }.buttonStyle(SwitcherChoiceStyle(selected: false, palette: palette))
            .help(label).accessibilityLabel(label).accessibilityIdentifier("workspace-switcher-options")
            .editorPopover(isPresented: $open) {
                if open {
                    EditorActionMenu(model: WorkspaceSwitcher.options(workspaces), width: 280,
                        identifier: "workspace-switcher-options-menu") { open = false }
                }
            }
            .onChange(of: open) { _, open in workspaces.store?.workspace.popover(popupID, open: open) }
            .onDisappear { workspaces.store?.workspace.popover(popupID, open: false) }
    }
}

/// Use the label's natural width up to the shared cap. A flexible max-width
/// frame fills the header's spare space and turns short names into wide cells.
private struct WorkspaceNameWidth: Layout {
    let natural: CGFloat
    let maximum: CGFloat
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let label = subviews.first else { return .zero }
        let width = max(0, min(maximum, natural, proposal.width ?? .infinity))
        // A truncated Text can report less than the proposed width after placing
        // its ellipsis. Retain the shared cell width so following labels align.
        return CGSize(width: width, height: label.sizeThatFits(ProposedViewSize(width: width, height: proposal.height)).height)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: CGPoint(x: bounds.midX, y: bounds.minY), anchor: .top, proposal: ProposedViewSize(bounds.size))
    }
}

private struct SwitcherChoiceStyle: ButtonStyle {
    let selected: Bool
    let palette: EditorPalette
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, style: self) }
    private struct Face: View {
        let configuration: Configuration
        let style: SwitcherChoiceStyle
        @State private var hovering = false
        var body: some View {
            configuration.label.background {
                if style.selected {
                    SquircleShape.tile.fill(hovering ? style.palette.headerSelectionHover : style.palette.glassSwitcherSelection)
                } else if hovering || configuration.isPressed {
                    SquircleShape.tile.fill(style.palette["text"].opacity(configuration.isPressed ? 0.16 : 0.08))
                }
            }.onHover { hovering = $0 }
        }
    }
}

private extension JSON { var switcherID: String { self["id"].string } }
