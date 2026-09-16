import SwiftUI
import UIKit
import QuartzCore

struct MetalCanvas: UIViewRepresentable {
    let store: EditorStore
    func makeUIView(context: Context) -> CanvasView { CanvasView(store: store) }
    func updateUIView(_ view: CanvasView, context: Context) {}
    static func dismantleUIView(_ view: CanvasView, coordinator: ()) { view.stop() }
}

final class CanvasView: UIView {
    override class var layerClass: AnyClass { ObservedMetalLayer.self }
    let store: EditorStore
    private var displayLink: CADisplayLink?
    private lazy var frames = CanvasFrameDriver(store: store)
    private var attached = false
    private var drawableExtent = CGSize.zero
    private var sceneGeometry: NSKeyValueObservation?
    private var workspaceBottom: CGFloat = -1
    var contacts: [ObjectIdentifier: PencilContact] = [:]
    var nextContact: UInt64 = 0
    var ignoredContacts: Set<ObjectIdentifier> = []
    var estimates = EstimatedInput()
    var modifiers: UIKeyModifierFlags = []

    init(store: EditorStore) {
        self.store = store
        super.init(frame: .zero)
        isMultipleTouchEnabled = true
        isOpaque = true
        backgroundColor = .clear
        accessibilityIdentifier = "canvas"
        isAccessibilityElement = true
        accessibilityLabel = "Canvas"
        accessibilityValue = "Initializing"
        let metal = layer as! CAMetalLayer
        metal.isOpaque = true
        metal.framebufferOnly = true
        metal.presentsWithTransaction = false
        metal.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(hovered(_:)))
        hover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.pencil.rawValue)]
        addGestureRecognizer(hover)
        installIndirectGestures()
        store.wake = { [weak self] in self?.wake() }
        store.focusCanvas = { [weak self] in
            guard let self, self.window?.isKeyWindow == true,
                self.window?.rootViewController?.presentedViewController == nil else { return }
            self.becomeFirstResponder()
        }
        store.interruptInput = { [weak self] in self?.interruptContacts() }
        frames.setPaused = { [weak self] paused in self?.displayLink?.isPaused = paused }
        frames.submittedViewport = { [weak self] in self?.accessibilityValue = "Metal ready" }
        NotificationCenter.default.addObserver(self, selector: #selector(keyboardChanged(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
    }
    required init?(coder: NSCoder) { fatalError("Use init(store:)") }
    override var canBecomeFirstResponder: Bool { true }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        sceneGeometry = nil
        if let window {
            measureWorkspaceBottom()
            store.systemSceneID = window.windowScene?.session.persistentIdentifier
            store.focusWindow = { [weak window] in
                guard let scene = window?.windowScene else { return }
                UIApplication.shared.requestSceneSessionActivation(scene.session, userActivity: nil, options: nil)
            }
            sceneGeometry = window.windowScene?.observe(\.effectiveGeometry, options: [.initial, .new]) { [weak self] scene, _ in
                DispatchQueue.main.async {
                    guard let self, self.window?.windowScene === scene else { return }
                    let space: any UICoordinateSpace
                    if #available(iOS 26.0, *) { space = scene.effectiveGeometry.coordinateSpace }
                    else { space = scene.coordinateSpace }
                    let display = scene.screen.coordinateSpace
                    self.store.windowPresentation.observe(fullscreen: space.convert(space.bounds, to: display) == display.bounds)
                }
            }
            store.projectFiles.closeWindow = { [weak window, weak store] in
                guard let store else { return }
                DocumentScene.close(window?.windowScene, store: store)
            }
            contentScaleFactor = window.screen.scale
            if displayLink == nil {
                let link = CADisplayLink(target: self, selector: #selector(tick(_:)))
                let maximum = Float(window.screen.maximumFramesPerSecond)
                link.preferredFrameRateRange = CAFrameRateRange(minimum: min(60, maximum), maximum: maximum, preferred: maximum)
                link.add(to: .main, forMode: .common)
                displayLink = link
            }
            becomeFirstResponder()
            setNeedsLayout()
        } else {
            store.focusWindow = nil
            if store.state["document_file"]["close_ready"].bool { store.recovery.close() }
            stop()
        }
    }
    override func layoutSubviews() {
        super.layoutSubviews()
        guard window != nil, bounds.width > 0, bounds.height > 0 else { return }
        measureWorkspaceBottom()
        let extent = CGSize(width: (bounds.width * contentScaleFactor).rounded(), height: (bounds.height * contentScaleFactor).rounded())
        guard extent != drawableExtent || !attached else { return }
        drawableExtent = extent
        let metal = layer as! CAMetalLayer
        metal.contentsScale = contentScaleFactor
        metal.drawableSize = extent
        let width = UInt32(extent.width), height = UInt32(extent.height)
        store.native?.observeDisplay(width: width, height: height, scale: Float(contentScaleFactor),
            maximumRefreshRate: window?.screen.maximumFramesPerSecond ?? 0)
        if !attached {
            store.native?.attach(metal, width: width, height: height, scale: Float(contentScaleFactor))
            attached = true
            frames.activate()
        } else { store.native?.resize(width: width, height: height, scale: Float(contentScaleFactor)) }
        wake()
    }
    @objc private func keyboardChanged(_ notification: Notification) {
        // Read UIKit's per-window guide after its layout update. Shared workspace
        // clearance moves controls above the keyboard without resizing the canvas.
        DispatchQueue.main.async { [weak self] in
            self?.window?.layoutIfNeeded()
            self?.measureWorkspaceBottom()
        }
    }
    private func measureWorkspaceBottom() {
        guard window != nil, bounds.height > 0 else { return }
        var minimum: CGFloat = 0
        if #available(iOS 26.0, *), traitCollection.userInterfaceIdiom == .pad { minimum = 36 }
        let keyboard = bounds.intersection(keyboardLayoutGuide.layoutFrame)
        let inset = max(minimum, keyboard.isNull ? 0 : keyboard.height)
        guard inset != workspaceBottom else { return }
        workspaceBottom = inset
        store.dispatch(["type": "measure_workspace_bottom", "inset": Double(inset)])
    }
    func stop() {
        frames.deactivate()
        store.input(["type": "blur"])
        displayLink?.invalidate(); displayLink = nil
        if attached { store.native?.detach(); attached = false }
        contacts.removeAll(); ignoredContacts.removeAll()
    }
    func wake() { frames.wake() }
    @objc private func tick(_ link: CADisplayLink) {
        frames.tick(target: link.targetTimestamp)
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 1) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 2) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 3) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 4) }
    override func touchesEstimatedPropertiesUpdated(_ touches: Set<UITouch>) { updateEstimates(touches) }
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        routeKeys(presses, pressed: true)
        super.pressesBegan(presses, with: event)
    }
    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        routeKeys(presses, pressed: false)
        super.pressesEnded(presses, with: event)
    }
    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        routeKeys(presses, pressed: false)
        super.pressesCancelled(presses, with: event)
    }
}
