import SwiftUI

/// Stable shared workspace identities; selecting a segment uses ordinary save,
/// switch and window-ownership policy rather than reapplying a shipped preset.
struct WorkspaceSwitcher: View {
    @ObservedObject var library: WorkspaceLibrary
    @ObservedObject var manager: WorkspaceManager
    let palette: EditorPalette
    var body: some View {
        HStack(spacing: 2) {
            ForEach(library.status["default_workspaces"].array, id: \.switcherID) { workspace in
                let selected = workspace["id"].string == library.status["active_id"].string
                Button { manager.activate(JSON(["type": "switch", "value": workspace["id"].raw])) } label: {
                    Text(workspace["name"].string).fontWeight(.medium).lineLimit(1)
                        .frame(maxWidth: 110).padding(.horizontal, 10).frame(height: 28)
                        .foregroundStyle(palette["text"])
                        .background {
                            if selected {
                                ZStack { Capsule().fill(palette["bg"]); Capsule().fill(Color.accentColor.opacity(0.28)) }
                            }
                        }
                }.buttonStyle(.plain)
                    .help("Switch to \(workspace["name"].string) workspace")
                    .accessibilityIdentifier("workspace-switch-" + workspace["id"].string)
                    .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }.padding(3)
            .background {
                ZStack { Capsule().fill(palette["bg"]); Capsule().fill(palette["text"].opacity(0.06)) }
            }
            .overlay(Capsule().strokeBorder(palette["text"].opacity(0.1), lineWidth: 1))
            .disabled(!library.ready || library.busy || library.readOnly || manager.processing)
            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-switcher")
    }
}

private extension JSON { var switcherID: String { self["id"].string } }
