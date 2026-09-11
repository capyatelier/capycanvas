import SwiftUI

/// System scene restoration retains each window's workspace identity. A new
/// scene receives its own identity and starts from the last committed layout.
struct EditorSessionScene<Content: View>: View {
    @SceneStorage("capy.editor.session") private var identifier = UUID().uuidString
    @ViewBuilder let content: (String) -> Content
    var body: some View {
        content(identifier).id(identifier)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("editor-scene-" + identifier)
    }
}
