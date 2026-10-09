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
    private var screenReport: ScreenReport?
    private var attached = false
    private lazy var frames = CanvasFrameDriver(store: store)
    private lazy var input = MacInput(view: self, store: store)
    private var tracking: NSTrackingArea?
    private var windowObservers: [NSObjectProtocol] = []
    private var shaderInputMonitor: Any?
    private lazy var documentDelegate = DocumentWindowDelegate(store: store)
    private var extent = CGSize.zero
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var isOpaque: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
    init(store: EditorStore) {
        self.store = store
        super.init(frame: .zero)
        let metal = ObservedMetalLayer()
        metal.isOpaque = true
        metal.colorspace = CGColorSpace(name: CGColorSpace.displayP3)
        // Assign first to host our Metal layer; AppKit must not draw its contents.
        layer = metal
        wantsLayer = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityIdentifier("canvas")
        setAccessibilityLabel(store.bootstrap["drawing_canvas"].string)
        setAccessibilityValue(store.bootstrap["starting_canvas"].string)
        store.bootstrapChanged = { [weak self] in self?.updateAccessibility() }
        updateAccessibility()
        store.wake = { [weak self] in self?.wake() }
        store.observeDisplayHeadroom = { [weak self] in self?.updateHeadroom() }
        store.interruptInput = { [weak self] in self?.input.interrupt() }
        store.focusCanvas = { [weak self] in
            guard let self, self.window?.isKeyWindow == true, self.window?.attachedSheet == nil else { return }
            self.window?.makeFirstResponder(self)
        }
        frames.canPresent = { [weak self] in self?.window?.occlusionState.contains(.visible) == true }
        frames.setPaused = { [weak self] paused in self?.displayLink?.isPaused = paused }
        frames.submittedViewport = { [weak self] in self?.setAccessibilityValue(self?.store.bootstrap["canvas_ready"].string) }
    }
    required init?(coder: NSCoder) { fatalError("Use init(store:)") }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        documentDelegate.attach(window)
        store.cursorChanged = { [weak self] in self?.input.applyCursor() }
        for observer in windowObservers { NotificationCenter.default.removeObserver(observer) }
        windowObservers.removeAll()
        if let shaderInputMonitor { NSEvent.removeMonitor(shaderInputMonitor) }
        shaderInputMonitor = nil
        if let window {
            shaderInputMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp,
                .rightMouseDown, .otherMouseDown, .mouseMoved, .scrollWheel, .magnify, .rotate, .keyDown, .keyUp, .tabletPoint]) { [weak self, weak window] event in
                if event.window === window { self?.store.native?.shaderInput() }
                return event
            }
            windowObservers.append(NotificationCenter.default.addObserver(forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.updateHeadroom() }
            })
            window.acceptsMouseMovedEvents = true
            window.makeFirstResponder(self)
            store.acceptsGamepad = { [weak window] in
                guard let window else { return false }
                return window.isKeyWindow || window.attachedSheet?.isKeyWindow == true
            }
            GamepadInput.shared.start()
            for name in [NSWindow.didResignKeyNotification, NSWindow.willCloseNotification] {
                windowObservers.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.store.input(["type": "blur"]) }
                })
            }
            windowObservers.append(NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    if self.window?.occlusionState.contains(.visible) == true {
                        self.store.native?.redraw()
                    }
                    self.wake()
                }
            })
            windowObservers.append(NotificationCenter.default.addObserver(forName: NSWindow.didChangeScreenNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.displayChanged() }
            })
            if displayLink == nil {
                let link = displayLink(target: self, selector: #selector(tick(_:)))
                link.add(to: .main, forMode: .common)
                displayLink = link
            }
            needsLayout = true
        } else { store.acceptsGamepad = nil; stop() }
    }
    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        displayChanged()
    }
    private func displayChanged() {
        needsLayout = true
        store.native?.redraw()
        wake()
    }
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .cursorUpdate, .activeInKeyWindow, .inVisibleRect], owner: self)
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
        let details = DisplayDetails(screen: window.screen?.localizedName ?? "Unknown screen",
            destination: window.screen?.colorSpace?.localizedName ?? "macOS color management")
        DispatchQueue.main.async { [weak store] in
            if store?.displayDetails != details { store?.displayDetails = details }
        }
        store.native?.observeDisplay(width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale),
            maximumRefreshRate: window.screen?.maximumFramesPerSecond ?? 0)
        guard next != extent || !attached else { return }
        extent = next; layer.contentsScale = scale; layer.drawableSize = next
        if !attached { store.native?.attach(layer, width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale)); attached = true; frames.activate() }
        else { store.native?.resize(width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale)) }
        wake()
    }
    func wake() { frames.wake() }
    private func updateAccessibility() {
        setAccessibilityLabel(store.bootstrap["drawing_canvas"].string)
        setAccessibilityHelp(store.bootstrap["drawing_canvas_help"].string)
        setAccessibilityValue(store.bootstrap[store.restartingCanvas ? "restarting_canvas" : store.canvasSubmitted ? "canvas_ready" : "starting_canvas"].string)
    }
    func stop() {
        documentDelegate.attach(nil)
        frames.deactivate(); store.input(["type": "blur"])
        for observer in windowObservers { NotificationCenter.default.removeObserver(observer) }
        windowObservers.removeAll()
        if let shaderInputMonitor { NSEvent.removeMonitor(shaderInputMonitor) }
        shaderInputMonitor = nil
        displayLink?.invalidate(); displayLink = nil
        if attached { store.native?.detach(); attached = false }
    }
    private func updateHeadroom() {
        let value = Double(window?.screen?.maximumExtendedDynamicRangeColorComponentValue ?? 1).clampedHeadroom
        if store.displayHeadroom != value {
            store.displayHeadroom = value; store.native?.displayHeadroom(value)
        }
        guard let screen = window?.screen else { return }
        let report = ScreenReport(name: screen.localizedName, wide: screen.canRepresent(.p3),
            headroom: Double(screen.maximumPotentialExtendedDynamicRangeColorComponentValue))
        if report != screenReport { screenReport = report; store.native?.screenReport(report) }
    }
    @objc private func tick(_ link: CADisplayLink) {
        updateHeadroom()
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
    override func mouseExited(with event: NSEvent) { input.leaveCanvasCursor(); input.clearHover() }
    override func cursorUpdate(with event: NSEvent) { input.applyCursor() }
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
        // Control-Command-F and other native Control-Command chords belong to
        // AppKit; the shared keymap represents Command and Control as one flag.
        guard !event.modifierFlags.contains(.control) else { return false }
        if ["q", "w", "n", "m", "h"].contains(event.charactersIgnoringModifiers?.lowercased() ?? "") { return false }
        // The shared keymap owns canvas shortcuts; focused native text editors
        // retain the system's command-key handling.
        input.key(event, pressed: true)
        input.key(event, pressed: false)
        return true
    }
}
