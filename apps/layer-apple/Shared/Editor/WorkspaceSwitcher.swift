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
        }
    }
    var body: some View {
        Group {
            if maximumWidth + 0.5 < naturalWidth {
                EditorMenuButton(menu: {
                    AppleContextMenu(Self.menu(workspaces)) { workspaces.switchTo($0["id"].string) }
                }, identifier: "workspace-switcher-menu") {
                    HStack(spacing: 8) {
                        Text(choices.first(where: { $0["current"].bool })?["title"].string ?? "Workspaces")
                            .lineLimit(1)
                        Image(systemName: "chevron.down").font(.system(size: 11))
                    }.padding(.horizontal, 12).frame(width: maximumWidth, height: tile)
                }.buttonStyle(HeaderButtonStyle(radius: tile / 2))
            } else {
                segments.glassSurface(SquircleShape.tile, fill: palette.glassSwitcher)
            }
        }
        .disabled(!workspaces.ready || workspaces.busy || workspaces.readOnly || workspaces.switcherBusy || workspaces.presented)
        .accessibilityElement(children: .contain).accessibilityLabel("Workspaces").accessibilityIdentifier("workspace-switcher")
        .modifier(HeaderControlMeasurement(id: "workspace-switcher"))
    }
    @MainActor static func menu(_ workspaces: WorkspaceController) -> JSON {
        JSON(["sections":[workspaces.view["switcher_display"].array.map { workspace in
            ["label":workspace["title"].raw, "enabled":workspaces.ready && !workspaces.busy && !workspaces.readOnly && !workspaces.switcherBusy,
                "selected":workspace["current"].bool,
                "action":["type":"apple_workspace_switch", "id":workspace["id"].raw]]
        }]])
    }
    private var segments: some View {
        WorkspaceNameWidth(natural: naturalWidth, maximum: maximumWidth) {
            ScrollViewReader { scroll in
                EditorScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 2) {
                        ForEach(choices, id: \.switcherID) { workspace in
                            choice(workspace).id(workspace["id"].string)
                        }
                    }.padding(5)
                }.frame(height: 36)
                    .onChange(of: choices.first?["id"].string) { _, first in
                        if let first, first == workspaces.view["id"].string { scroll.scrollTo(first, anchor: .leading) }
                    }
            }
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
