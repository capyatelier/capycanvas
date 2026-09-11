import SwiftUI
import AppKit
import QuartzCore

struct MacMetalCanvas: NSViewRepresentable {
    let store: EditorStore
    func makeNSView(context: Context) -> MacCanvasView { MacCanvasView(store: store) }
    func updateNSView(_ view: MacCanvasView, context: Context) {}
    static func dismantleNSView(_ view: MacCanvasView, coordinator: ()) { view.stop() }
}

/// Native AppKit presentation foundation; desktop input has its own adapter.
final class MacCanvasView: NSView {
    let store: EditorStore
    private var displayLink: CADisplayLink?
    private var attached = false
    private lazy var frames = CanvasFrameDriver(store: store)
    private lazy var input = MacInput(view: self, store: store)
    private var tracking: NSTrackingArea?
    private var windowObservers: [NSObjectProtocol] = []
    private var extent = CGSize.zero
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var isOpaque: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
    init(store: EditorStore) {
        self.store = store
        super.init(frame: .zero)
        wantsLayer = true
        let metal = ObservedMetalLayer()
        metal.isOpaque = true
        metal.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        layer = metal
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("canvas")
        setAccessibilityLabel("Canvas")
        setAccessibilityValue("Initializing")
        store.wake = { [weak self] in self?.wake() }
        frames.setPaused = { [weak self] paused in self?.displayLink?.isPaused = paused }
        frames.submittedViewport = { [weak self] in self?.setAccessibilityValue("Metal ready") }
    }
    required init?(coder: NSCoder) { fatalError("Use init(store:)") }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        for observer in windowObservers { NotificationCenter.default.removeObserver(observer) }
        windowObservers.removeAll()
        if let window {
            window.acceptsMouseMovedEvents = true
            window.makeFirstResponder(self)
            for name in [NSWindow.didResignKeyNotification, NSWindow.willCloseNotification] {
                windowObservers.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.input.blur() }
                })
            }
            windowObservers.append(NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.wake() }
            })
            if displayLink == nil {
                let link = displayLink(target: self, selector: #selector(tick(_:)))
                link.add(to: .main, forMode: .common)
                displayLink = link
            }
            needsLayout = true
        } else { stop() }
    }
    override func viewDidChangeBackingProperties() { super.viewDidChangeBackingProperties(); needsLayout = true }
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area); tracking = area
    }
    override func layout() {
        super.layout()
        guard let window, let layer = layer as? CAMetalLayer, bounds.width > 0, bounds.height > 0 else { return }
        let controls = [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
        let inset = controls.compactMap { window.standardWindowButton($0) }
            .filter { !$0.isHidden }.map { convert($0.bounds, from: $0).maxX }.max() ?? 0
        if store.headerLeadingInset != inset {
            // SwiftUI must receive platform measurements outside its layout pass.
            DispatchQueue.main.async { [weak self] in self?.store.headerLeadingInset = inset }
        }
        let scale = window.backingScaleFactor
        let next = CGSize(width: (bounds.width * scale).rounded(), height: (bounds.height * scale).rounded())
        guard next != extent || !attached else { return }
        extent = next; layer.contentsScale = scale; layer.drawableSize = next
        store.native?.observeDisplay(width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale),
            maximumRefreshRate: window.screen?.maximumFramesPerSecond ?? 0)
        if !attached { store.native?.attach(layer, width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale)); attached = true; frames.activate() }
        else { store.native?.resize(width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale)) }
        wake()
    }
    func wake() { frames.wake() }
    func stop() {
        frames.deactivate(); input.blur()
        for observer in windowObservers { NotificationCenter.default.removeObserver(observer) }
        windowObservers.removeAll()
        displayLink?.invalidate(); displayLink = nil
        if attached { store.native?.detach(); attached = false }
    }
    @objc private func tick(_ link: CADisplayLink) {
        frames.tick(target: link.targetTimestamp)
    }
    override func mouseDown(with event: NSEvent) { input.mouse(event, phase: 1) }
    override func mouseDragged(with event: NSEvent) { input.mouse(event, phase: 2) }
    override func mouseUp(with event: NSEvent) { input.mouse(event, phase: 3) }
    override func rightMouseDown(with event: NSEvent) { input.mouse(event, phase: 1) }
    override func rightMouseDragged(with event: NSEvent) { input.mouse(event, phase: 2) }
    override func rightMouseUp(with event: NSEvent) { input.mouse(event, phase: 3) }
    override func otherMouseDown(with event: NSEvent) { input.mouse(event, phase: 1) }
    override func otherMouseDragged(with event: NSEvent) { input.mouse(event, phase: 2) }
    override func otherMouseUp(with event: NSEvent) { input.mouse(event, phase: 3) }
    override func mouseMoved(with event: NSEvent) { input.hover(event) }
    override func mouseExited(with event: NSEvent) { input.clearHover() }
    override func tabletPoint(with event: NSEvent) { input.tabletPoint(event) }
    override func tabletProximity(with event: NSEvent) { input.proximity(event) }
    override func scrollWheel(with event: NSEvent) { input.scroll(event) }
    override func magnify(with event: NSEvent) { input.gesture(event, rotate: false) }
    override func rotate(with event: NSEvent) { input.gesture(event, rotate: true) }
    override func keyDown(with event: NSEvent) { input.key(event, pressed: true) }
    override func keyUp(with event: NSEvent) { input.key(event, pressed: false) }
    override func flagsChanged(with event: NSEvent) { input.updateModifiers(event.modifierFlags) }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, event.modifierFlags.contains(.command) else { return false }
        if ["q", "w", "n", "m", "h"].contains(event.charactersIgnoringModifiers?.lowercased() ?? "") { return false }
        // The shared keymap owns canvas shortcuts; focused native text editors
        // retain the system's command-key handling.
        input.key(event, pressed: true)
        input.key(event, pressed: false)
        return true
    }
}
