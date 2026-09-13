import SwiftUI

/// Keep text on an opaque surface from the same shared palette. Drawing content
/// must not change the contrast of a popup while it is open.
struct EditorPopupSurface<S: Shape>: ViewModifier {
    let shape: S
    @Environment(\.editorPopupStore) private var store
    private var source: JSON { store?.state["palette"] ?? JSON() }

    @ViewBuilder func body(content: Content) -> some View {
        if !source["panel"].isNull && !source["text"].isNull {
            let palette = EditorPalette(source: source)
            content.foregroundStyle(palette["text"]).background(palette["panel"], in: shape)
        } else {
            content.foregroundStyle(.primary).background(.background, in: shape)
        }
    }
}

private struct EditorPopupStoreKey: EnvironmentKey {
    static let defaultValue: EditorStore? = nil
}
extension EnvironmentValues {
    var editorPopupStore: EditorStore? {
        get { self[EditorPopupStoreKey.self] }
        set { self[EditorPopupStoreKey.self] = newValue }
    }
}

/// Set both sides of the color pair, including the popover arrow and sheet
/// margins that a content-only background would leave translucent.
struct EditorPopupPresentation: ViewModifier {
    @Environment(\.editorPopupStore) private var store
    private var source: JSON { store?.state["palette"] ?? JSON() }
    @ViewBuilder func body(content: Content) -> some View {
        if !source["panel"].isNull && !source["text"].isNull {
            let palette = EditorPalette(source: source)
            content.modifier(EditorPopoverHost()).foregroundStyle(palette["text"]).presentationBackground(palette["panel"])
                .modifier(EditorPresentationAppearance())
        } else {
            content.modifier(EditorPopoverHost()).foregroundStyle(.primary).presentationBackground(.background)
        }
    }
}

/// A presented sheet keeps its own appearance; read the live resolved theme
/// instead of retaining the environment value captured when it opened.
struct EditorPresentationAppearance: ViewModifier {
    @Environment(\.editorPopupStore) private var store
    func body(content: Content) -> some View {
        content.preferredColorScheme(store.map { $0.state["theme"].string == "dark" ? .dark : .light })
    }
}
