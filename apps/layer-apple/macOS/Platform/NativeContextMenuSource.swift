import AppKit
import SwiftUI

extension View {
    func nativeEditorContextMenu(identity: String, load: @escaping AppleContextMenuRequest,
        visibility: @escaping (Bool) -> Void) -> some View {
        overlay(NativeContextMenuInput(identity: identity, load: load, visibility: visibility))
    }
}

private struct NativeContextMenuInput: NSViewRepresentable {
    let identity: String
    let load: AppleContextMenuRequest
    let visibility: (Bool) -> Void
    func makeNSView(context: Context) -> NativeContextMenuInputView { NativeContextMenuInputView() }
    func updateNSView(_ view: NativeContextMenuInputView, context: Context) {
        if view.identity != identity { view.invalidate() }
        view.identity = identity; view.load = load; view.visibility = visibility
    }
    static func dismantleNSView(_ view: NativeContextMenuInputView, coordinator: ()) { view.invalidate() }
}

final class NativeContextMenuInputView: NSView {
    var identity = ""
    var load: AppleContextMenuRequest = { $0(nil) }
    var visibility: (Bool) -> Void = { _ in }
    private var request = UUID()
    private var presentedMenu: NSMenu?
    private var timer: Timer?
    private var focusObserver: NSObjectProtocol?
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard NSApp.currentEvent?.type == .rightMouseDown else { return nil }
        return super.hitTest(point)
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        invalidate()
        if let focusObserver { NotificationCenter.default.removeObserver(focusObserver) }
        focusObserver = window.map { window in
            NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification,
                object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.invalidate() }
            }
        }
    }
    deinit { if let focusObserver { NotificationCenter.default.removeObserver(focusObserver) } }
    func invalidate() {
        request = UUID(); timer?.invalidate(); timer = nil
        if let presentedMenu { self.presentedMenu = nil; presentedMenu.cancelTracking(); visibility(false) }
    }
    override func rightMouseDown(with event: NSEvent) {
        invalidate()
        let ticket = UUID(); request = ticket
        let point = convert(event.locationInWindow, from: nil)
        load { [weak self] model in
            guard let self, self.request == ticket, let model, self.window?.isKeyWindow == true else { return }
            let native = model.nativeMenu()
            // Enter native tracking outside a SwiftUI update or a running main
            // queue task, so editor publications can continue while it is open.
            let timer = Timer(timeInterval: 0, repeats: false) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, self.request == ticket, self.window?.isKeyWindow == true else { return }
                    self.timer = nil; self.presentedMenu = native; self.visibility(true)
                    native.popUp(positioning: nil, at: point, in: self)
                    if self.request == ticket { self.presentedMenu = nil; self.visibility(false) }
                }
            }
            self.timer = timer; RunLoop.main.add(timer, forMode: .common)
        }
    }
}
