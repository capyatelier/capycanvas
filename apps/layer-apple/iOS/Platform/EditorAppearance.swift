import SwiftUI
import UIKit

/// Keep the window choice separate from the scene's system appearance. Native
/// menus inherit the window; Auto must still observe the underlying OS choice.
struct NativeEditorAppearance: UIViewRepresentable {
    let store: EditorStore
    let preferred: String
    func makeUIView(context: Context) -> EditorAppearanceView { EditorAppearanceView() }
    func updateUIView(_ view: EditorAppearanceView, context: Context) {
        view.store = store; view.preferred = preferred; view.update()
    }
}
final class EditorAppearanceView: UIView {
    weak var store: EditorStore?
    var preferred = ""
    private weak var observedScene: UIWindowScene?
    private var registration: (any UITraitChangeRegistration)?
    private var reported: UIUserInterfaceStyle?
    override init(frame: CGRect) { super.init(frame: frame); isUserInteractionEnabled = false }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    override func didMoveToWindow() { super.didMoveToWindow(); update() }
    func update() {
        if observedScene !== window?.windowScene {
            if let registration { observedScene?.unregisterForTraitChanges(registration) }
            observedScene = window?.windowScene; reported = nil
            registration = observedScene?.registerForTraitChanges([UITraitUserInterfaceStyle.self]) {
                [weak self] (scene: UIWindowScene, _: UITraitCollection) in self?.report(scene.traitCollection.userInterfaceStyle)
            }
        }
        let choice: UIUserInterfaceStyle = preferred == "dark" ? .dark : preferred == "light" ? .light : .unspecified
        if window?.overrideUserInterfaceStyle != choice { window?.overrideUserInterfaceStyle = choice }
        if let scene = observedScene { report(scene.traitCollection.userInterfaceStyle) }
    }
    private func report(_ style: UIUserInterfaceStyle) {
        guard style != .unspecified, style != reported, let store else { return }
        reported = style
        store.dispatch(["type": "system_theme_changed", "theme": style == .dark ? "dark" : "light"])
    }
}
