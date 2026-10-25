import SwiftUI

struct EditorFittedPresentation: ViewModifier {
    @State private var height: CGFloat = 500
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 18.0, macOS 15.0, *) {
            content.presentationSizing(.fitted)
        } else {
            content.fixedSize(horizontal: false, vertical: true)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height = max(1, $0) }
                .presentationDetents([.height(height)])
        }
    }
}

struct EditorDialogPresentation: ViewModifier {
    let identifier: String
    var idealHeight: CGFloat = 500
    func body(content: Content) -> some View {
        content.frame(minWidth: 380, idealWidth: 800, maxWidth: 860, minHeight: 420, idealHeight: idealHeight, maxHeight: 620)
            .accessibilityElement(children: .contain).accessibilityIdentifier(identifier)
            .modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
    }
}

struct EditorDialogTitle: View {
    let text: String
    var body: some View { Text(text).font(.headline).frame(maxWidth: .infinity) }
}

struct EditorDialogFooter<Leading: View>: View {
    @Environment(\.capyCommonCopy) private var commonCopy
    @Environment(\.editorPopupStore) private var store
    let label: String
    let identifiers: (cancel: String, apply: String)
    var cancelsOnEscape = true
    var blocked = false
    let cancel: () -> Void
    let apply: () -> Void
    @ViewBuilder let leading: () -> Leading
    private var palette: EditorPalette { EditorPalette(source: store?.state["palette"] ?? JSON()) }
    var body: some View {
        VStack(spacing: 0) {
            EditorLine(color: palette.settingsBorder)
            HStack(spacing: 6) {
                leading()
                Button(commonCopy["cancel"].string, action: cancel).buttonStyle(.bordered)
                    .keyboardShortcut(cancelsOnEscape ? .cancelAction : nil).accessibilityIdentifier(identifiers.cancel)
                Button(label, action: apply)
                    .buttonStyle(.borderedProminent).tint(palette.accent)
                    .disabled(blocked).keyboardShortcut(.defaultAction).accessibilityIdentifier(identifiers.apply)
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }
    }
}

/// Keep text on an opaque surface from the same shared palette. Drawing content
/// must not change the contrast of a popup while it is open.
struct EditorPopupSurface<S: Shape>: ViewModifier {
    let shape: S
    @Environment(\.editorPopupStore) private var store
    private var source: JSON { store?.state["palette"] ?? JSON() }

    @ViewBuilder func body(content: Content) -> some View {
        if !source["panel"].isNull && !source["text"].isNull {
            let palette = EditorPalette(source: source)
            content.foregroundStyle(palette["text"]).background(palette["panel"], in: shape).environment(\.onSettingsSurface, false)
        } else {
            content.foregroundStyle(.primary).background(.background, in: shape).environment(\.onSettingsSurface, false)
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
    @State private var popup = UUID()
    private var source: JSON { store?.state["palette"] ?? JSON() }
    func body(content: Content) -> some View {
        Group {
            if !source["settings"].isNull && !source["text"].isNull {
                let palette = EditorPalette(source: source)
                content.modifier(EditorPopoverHost()).foregroundStyle(palette["text"]).presentationBackground(palette["settings"])
                    .environment(\.onSettingsSurface, true).modifier(EditorPresentationAppearance())
            } else {
                content.modifier(EditorPopoverHost()).foregroundStyle(.primary).presentationBackground(.background)
            }
        }
        .onAppear { store?.workspace.popover(popup, open: true, defersLanguage: false) }
        .onDisappear { store?.workspace.popover(popup, open: false) }
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
