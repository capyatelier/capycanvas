import SwiftUI

/// System scene restoration retains each window's workspace identity. A new
/// scene gets a distinct owner; the library chooses an available saved workspace.
struct EditorSessionScene<Content: View>: View {
    @SceneStorage("capy.editor.session") private var identifier: String?
    @ViewBuilder let content: (String) -> Content
    var body: some View {
        ZStack {
            if let identifier {
                content(identifier).id(identifier)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("editor-scene-" + identifier)
            }
        }.task {
            // Resolve restored scene storage before constructing an owner.
            // An eager UUID can otherwise open a different workspace first.
            if identifier == nil { identifier = UUID().uuidString }
        }
    }
}
