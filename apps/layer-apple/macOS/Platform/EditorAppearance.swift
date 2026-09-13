import SwiftUI
import AppKit

/// NSApplication supplies the system appearance independently of each editor
/// window's explicit choice. Clearing the window override restores Auto.
struct NativeEditorAppearance: NSViewRepresentable {
    let store: EditorStore
    let preferred: String
    func makeNSView(context: Context) -> EditorAppearanceView { EditorAppearanceView() }
    func updateNSView(_ view: EditorAppearanceView, context: Context) {
        view.store = store; view.preferred = preferred; view.update()
    }
}
final class EditorAppearanceView: NSView {
    weak var store: EditorStore?
    var preferred = ""
    private var observation: NSKeyValueObservation?
    private var reported: Bool?
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); update() }
    func update() {
        guard let window else { observation = nil; reported = nil; return }
        let name: NSAppearance.Name? = preferred == "dark" ? .darkAqua : preferred == "light" ? .aqua : nil
        if window.appearance?.name != name { window.appearance = name.flatMap(NSAppearance.init(named:)) }
        if observation == nil {
            observation = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
                DispatchQueue.main.async { self?.report() }
            }
        }
        report()
    }
    private func report() {
        guard window != nil, let store else { return }
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        guard dark != reported else { return }; reported = dark
        store.dispatch(["type": "system_theme_changed", "theme": dark ? "dark" : "light"])
    }
}
