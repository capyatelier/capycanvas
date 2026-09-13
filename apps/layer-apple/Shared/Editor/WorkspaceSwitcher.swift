import SwiftUI

/// Stable shared workspace identities; selecting a segment uses ordinary save,
/// switch and window-ownership policy rather than reapplying a shipped preset.
struct WorkspaceSwitcher: View {
    @ObservedObject var library: WorkspaceLibrary
    @ObservedObject var manager: WorkspaceManager
    let palette: EditorPalette
    var compact = false
    var textSize: Double = 44.0 / 3
    var maximumWidth: CGFloat = 420
    private var choices: [JSON] { library.status["switcher_display"].array }
    private var naturalWidth: CGFloat {
        8 + CGFloat(max(0, choices.count - 1)) * 2 + choices.reduce(0) { width, workspace in
            width + min(compact ? 80 : 110, HeaderTextMetrics.width(workspace["name"].string, size: textSize, weight: .medium))
                + (compact ? 10 : 20)
        }
    }
    var body: some View {
        WorkspaceNameWidth(natural: naturalWidth, maximum: maximumWidth) {
            ScrollViewReader { scroll in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 2) {
                        ForEach(choices, id: \.switcherID) { workspace in
                            choice(workspace).id(workspace["id"].string)
                        }
                    }.padding(4)
                }.frame(height: 34)
                    .onChange(of: choices.first?["id"].string) { _, first in
                        if let first, first == library.status["active_id"].string { scroll.scrollTo(first, anchor: .leading) }
                    }
            }
        }
        .clipShape(Capsule())
        .modifier(EditorGlassSurface(shape: Capsule()))
        .disabled(!library.ready || library.busy || library.readOnly || library.switcherBusy || manager.processing || manager.presented)
        .accessibilityElement(children: .contain).accessibilityLabel("Workspaces").accessibilityIdentifier("workspace-switcher")
        .modifier(HeaderControlMeasurement(id: "workspace-switcher"))
    }
    private func choice(_ workspace: JSON) -> some View {
        let selected = workspace["id"].string == library.status["active_id"].string
        return Button { manager.activate(JSON(["type": "switch", "value": workspace["id"].raw])) } label: {
            WorkspaceNameWidth(natural: HeaderTextMetrics.width(workspace["name"].string, size: textSize, weight: .medium),
                maximum: compact ? 80 : 110) {
                Text(workspace["name"].string).font(.system(size: textSize, weight: .medium)).lineLimit(1)
            }.padding(.horizontal, compact ? 5 : 10).frame(height: 26)
                .foregroundStyle(palette["text"])
                .background {
                    if selected {
                        ZStack { Capsule().fill(palette["bg"]); Capsule().fill(palette.accent.opacity(0.28)) }
                    }
                }
        }.buttonStyle(.plain).fixedSize(horizontal: true, vertical: false)
            .help("Switch to \(workspace["name"].string) workspace")
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

private extension JSON { var switcherID: String { self["id"].string } }
