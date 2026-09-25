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
    private var accentObserver: NSObjectProtocol?
    private var reported: String?
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); update() }
    func update() {
        guard let window else { observation = nil; accentObserver = nil; reported = nil; return }
        let name: NSAppearance.Name? = preferred == "dark" ? .darkAqua : preferred == "light" ? .aqua : nil
        if window.appearance?.name != name { window.appearance = name.flatMap(NSAppearance.init(named:)) }
        if observation == nil {
            observation = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
                DispatchQueue.main.async { self?.report() }
            }
        }
        if accentObserver == nil {
            accentObserver = NotificationCenter.default.addObserver(forName: NSColor.systemColorsDidChangeNotification,
                object: nil, queue: .main) { [weak self] _ in MainActor.assumeIsolated { self?.report() } }
        }
        report()
    }
    private func report() {
        guard window != nil, let store else { return }
        let dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let accent = NSColor.controlAccentColor.usingColorSpace(.sRGB).map { color in
            String(format: "#%02x%02x%02x", Int((color.redComponent * 255).rounded()),
                Int((color.greenComponent * 255).rounded()), Int((color.blueComponent * 255).rounded()))
        }
        let key = "\(dark):\(accent ?? "")"
        guard key != reported else { return }; reported = key
        var action: [String: Any] = ["type": "system_theme_changed", "theme": dark ? "dark" : "light"]
        if let accent { action["accent"] = accent }
        store.dispatch(action)
    }
}
