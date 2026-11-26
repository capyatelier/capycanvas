import SwiftUI
import UIKit
import UIKit.UIGestureRecognizerSubclass
import QuartzCore

struct MetalCanvas: UIViewRepresentable {
    let store: EditorStore
    @Environment(\.scenePhase) private var phase
    func makeUIView(context: Context) -> CanvasView {
        let view = CanvasView(store: store)
        view.setSceneActive(phase == .active)
        return view
    }
    func updateUIView(_ view: CanvasView, context: Context) { view.setSceneActive(phase == .active) }
    static func dismantleUIView(_ view: CanvasView, coordinator: ()) { view.stop() }
}

final class CanvasView: UIView {
    private let metal = ObservedMetalLayer()
    let store: EditorStore
    private var displayLink: CADisplayLink?
    private var screenReport: ScreenReport?
    private var phoneMetricsKey = ""
    private weak var phoneMetricsOwner: NativeOwner?
    private static let longPress = UILongPressGestureRecognizer()
    private lazy var frames = CanvasFrameDriver(store: store)
    private var sceneActive = false
    private var attached = false
    private var drawableExtent = CGSize.zero
    private var submittedScale: CGFloat = 0
    private var sceneGeometry: NSKeyValueObservation?
    private weak var factsWindow: UIWindow?
    private var keyboardScreenFrame: CGRect?
    private var keyboardCoversField = false
    private let shaderActivity = ShaderActivityRecognizer()
    private lazy var calibrationHover: UIHoverGestureRecognizer = {
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(calibrationHovered(_:)))
        hover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.pencil.rawValue)]
        hover.cancelsTouchesInView = false; hover.delegate = shaderActivity
        return hover
    }()
    private var workspaceBottom: CGFloat = -1
    var contacts: [ObjectIdentifier: PencilContact] = [:]
    var indirectGestures = Set<ObjectIdentifier>()
    var nextContact: UInt64 = 0
    var ignoredContacts: Set<ObjectIdentifier> = []
    var sensorContacts: Set<ObjectIdentifier> = []
    var pickerHold: PickerHold?
    var estimates = EstimatedInput()
    var modifiers: UIKeyModifierFlags = []
    var keyRepeat: KeyRepeat?

    init(store: EditorStore) {
        self.store = store
        super.init(frame: .zero)
        registerForTraitChanges([UITraitDisplayGamut.self, UITraitDisplayScale.self]) { (view: CanvasView, _: UITraitCollection) in
            view.setNeedsLayout()
            view.store.native?.redraw()
            view.wake()
        }
        isMultipleTouchEnabled = true
        isOpaque = true
        backgroundColor = .clear
        accessibilityIdentifier = "canvas"
        isAccessibilityElement = true
        accessibilityLabel = store.bootstrap["drawing_canvas"].string
        accessibilityValue = store.bootstrap["starting_canvas"].string
        metal.isOpaque = true
        metal.presentsWithTransaction = false
        layer.addSublayer(metal)
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(hovered(_:)))
        hover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.pencil.rawValue)]
        addGestureRecognizer(hover)
        let mouseHover = UIHoverGestureRecognizer(target: self, action: #selector(mouseHovered(_:)))
        mouseHover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.indirectPointer.rawValue)]
        addGestureRecognizer(mouseHover)
        let pointer = UIPointerInteraction(delegate: self)
        addInteraction(pointer)
        addInteraction(UIPencilInteraction(delegate: self))
        installIndirectGestures()
        updateAccessibility()
        frames.canPresent = { [weak self] in self?.sceneActive == true && self?.window != nil }
        frames.setPaused = { [weak self] paused in self?.displayLink?.isPaused = paused }
        frames.submittedViewport = { [weak self] in self?.updateAccessibility() }
        NotificationCenter.default.addObserver(self, selector: #selector(keyboardChanged(_:)),
            name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
    }
    private func connectStore() {
        store.phoneMetricsChanged = { [weak self] in self?.reportPhoneMetrics() }
        store.cursorChanged = { [weak self] in self?.interactions.compactMap { $0 as? UIPointerInteraction }.first?.invalidate() }
        store.bootstrapChanged = { [weak self] in self?.updateAccessibility() }
        updateAccessibility()
        store.wake = { [weak self] in self?.wake() }
        store.observeDisplayHeadroom = { [weak self] in self?.updateHeadroom() }
        store.focusCanvas = { [weak self] in
            guard let self, self.window?.isKeyWindow == true,
                self.window?.rootViewController?.presentedViewController == nil else { return }
            self.becomeFirstResponder()
        }
        store.interruptInput = { [weak self] in self?.interruptContacts() }
    }
    required init?(coder: NSCoder) { fatalError("Use init(store:)") }
    override var canBecomeFirstResponder: Bool { true }
    override var editingInteractionConfiguration: UIEditingInteractionConfiguration { .none }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        store.projectFiles.presentationScreen = window?.screen
        shaderActivity.view?.removeGestureRecognizer(shaderActivity)
        calibrationHover.view?.removeGestureRecognizer(calibrationHover)
        if window == nil { cancelPickerHold() }
        if let window {
            if factsWindow !== window { keyboardScreenFrame = nil; keyboardCoversField = false }
            factsWindow = window
            sceneGeometry = nil
            store.start(on: ProjectFiles.screenInfo(window.screen).map { JSON($0) })
            connectStore()
            shaderActivity.activity = { [weak store] in store?.native?.shaderInput() }
            shaderActivity.samples = { [weak self] touches, event, phase in self?.observeCalibration(touches, event: event, phase: phase) }
            window.addGestureRecognizer(shaderActivity)
            window.addGestureRecognizer(calibrationHover)
            measureWorkspaceBottom()
            store.systemSceneID = window.windowScene?.session.persistentIdentifier
            store.focusWindow = { [weak window] in
                guard let scene = window?.windowScene else { return }
                UIApplication.shared.requestSceneSessionActivation(scene.session, userActivity: nil, options: nil)
            }
            sceneGeometry = window.windowScene?.observe(\.effectiveGeometry, options: [.initial, .new]) { [weak self] scene, _ in
                DispatchQueue.main.async {
                    guard let self, self.factsWindow?.windowScene === scene else { return }
                    let space: any UICoordinateSpace
                    if #available(iOS 26.0, *) { space = scene.effectiveGeometry.coordinateSpace }
                    else { space = scene.coordinateSpace }
                    let display = scene.screen.coordinateSpace
                    self.store.windowPresentation.observe(fullscreen: space.convert(space.bounds, to: display) == display.bounds)
                    guard let window = self.factsWindow else { return }
                    let scale = window.screen.scale
                    let extent = CGSize(width: (window.bounds.width * scale).rounded(), height: (window.bounds.height * scale).rounded())
                    if extent.width > 0 && extent.height > 0 && (extent != self.drawableExtent || scale != self.submittedScale) {
                        self.drawableExtent = extent; self.submittedScale = scale
                        self.store.native?.observeDisplay(width: UInt32(extent.width), height: UInt32(extent.height), scale: Float(scale),
                            maximumRefreshRate: window.screen.maximumFramesPerSecond)
                        self.store.native?.resize(width: UInt32(extent.width), height: UInt32(extent.height), scale: Float(scale))
                    }
                    self.reportPhoneMetrics()
                    self.measureWorkspaceBottom()
                }
            }
            store.projectFiles.closeWindow = { [weak window, weak store] in
                guard let store else { return }
                DocumentScene.close(window?.windowScene, store: store)
            }
            contentScaleFactor = window.screen.scale
            store.acceptsGamepad = { [weak window] in
                window?.isKeyWindow == true && window?.windowScene?.activationState == .foregroundActive
            }
            GamepadInput.shared.start()
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
            store.acceptsGamepad = nil
            stop()
        }
    }
    override func layoutSubviews() {
        super.layoutSubviews()
        guard let window, bounds.width > 0, bounds.height > 0 else { return }
        contentScaleFactor = window.screen.scale
        // Keep the shared header clear of iPadOS window controls without
        // insetting the canvas or changing its input coordinates.
        let headerInset: CGFloat
        if #available(iOS 26.0, *) {
            headerInset = edgeInsets(for: .safeArea(cornerAdaptation: .horizontal)).left
        } else { headerInset = safeAreaInsets.left }
        DispatchQueue.main.async { [weak store] in
            if store?.headerLeadingInset != headerInset { store?.headerLeadingInset = headerInset }
        }
        measureWorkspaceBottom()
        reportPhoneMetrics()
        let details = DisplayDetails(screen: traitCollection.displayGamut == .P3 ? "Wide color (P3)" : "Standard color (sRGB)",
            destination: "iPadOS color management")
        DispatchQueue.main.async { [weak store] in
            if store?.displayDetails != details { store?.displayDetails = details }
        }
        let extent = CGSize(width: (bounds.width * contentScaleFactor).rounded(), height: (bounds.height * contentScaleFactor).rounded())
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        metal.frame = bounds
        metal.contentsScale = contentScaleFactor
        CATransaction.commit()
        guard extent != drawableExtent || contentScaleFactor != submittedScale || !attached else { return }
        drawableExtent = extent; submittedScale = contentScaleFactor
        let width = UInt32(extent.width), height = UInt32(extent.height)
        store.native?.observeDisplay(width: width, height: height, scale: Float(contentScaleFactor),
            maximumRefreshRate: window.screen.maximumFramesPerSecond)
        if !attached {
            store.native?.attach(metal, width: width, height: height, scale: Float(contentScaleFactor))
            attached = true
            frames.activate()
        } else { store.native?.resize(width: width, height: height, scale: Float(contentScaleFactor)) }
        store.native?.touchPolicy(milliseconds: UInt32(Self.longPress.minimumPressDuration * 1000),
            slop: Float(Self.longPress.allowableMovement * contentScaleFactor))
        wake()
    }
    private func reportPhoneMetrics() {
        guard let window = factsWindow, let owner = store.native, store.state["workspace_ready"].bool else { return }
        let edge = store.snapshot["layout"]["phone"]["edge"].string
        var insets: UIEdgeInsets
        if #available(iOS 26.0, *), !edge.isEmpty {
            insets = window.edgeInsets(for: .safeArea(cornerAdaptation: edge == "top" ? .horizontal : .vertical))
        } else { insets = window.safeAreaInsets }
        if traitCollection.userInterfaceIdiom == .phone {
            let orientation = window.windowScene?.interfaceOrientation
            let navigation = orientation == .portraitUpsideDown ? 0 : window.safeAreaInsets.bottom
            if store.navigationInset != navigation {
                DispatchQueue.main.async { [weak store = store] in
                    if store?.navigationInset != navigation { store?.navigationInset = navigation }
                }
            }
            switch orientation {
            case .landscapeLeft: insets = UIEdgeInsets(top: 0, left: 0, bottom: 0, right: insets.right)
            case .landscapeRight: insets = UIEdgeInsets(top: 0, left: insets.left, bottom: 0, right: 0)
            case .portraitUpsideDown: insets = UIEdgeInsets(top: 0, left: 0, bottom: insets.bottom, right: 0)
            default: insets = UIEdgeInsets(top: insets.top, left: 0, bottom: 0, right: 0)
            }
        }
        let safe = window.safeAreaInsets
        let metrics = JSON(["kind": "metrics", "insets": [insets.left, insets.top, insets.right, insets.bottom],
            "safe_area": [safe.left, safe.top, safe.right, safe.bottom],
            "corners": [0, 0, 0, 0], "native": [0, 0], "native_counts": [0, 0], "movable": false,
            "keyboard": keyboardFrame?.height ?? 0])
        guard phoneMetricsOwner !== owner || phoneMetricsKey != metrics.stableKey else { return }
        phoneMetricsOwner = owner; phoneMetricsKey = metrics.stableKey
        store.dispatch(["type": "phone", "action": metrics.raw])
    }
    override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        setNeedsLayout()
    }
    @objc private func keyboardChanged(_ notification: Notification) {
        guard let window = factsWindow else { return }
        let field = FocusedResponder.find() as? UIView
        keyboardScreenFrame = field?.window != nil && field?.window !== window ? nil
            : (notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? NSValue)?.cgRectValue
        reportPhoneMetrics()
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            factsWindow?.layoutIfNeeded()
            if let keyboard = keyboardFrame {
                keyboardCoversField = keyboardCoversField || (focusedFieldBottom ?? 0) > keyboard.minY
            } else { keyboardCoversField = false }
            measureWorkspaceBottom()
        }
    }
    private var keyboardFrame: CGRect? {
        guard let window = factsWindow, let screenFrame = keyboardScreenFrame else { return nil }
        let frame = window.screen.coordinateSpace.convert(screenFrame, to: window)
        guard frame.minX <= window.bounds.minX, frame.maxX >= window.bounds.maxX,
            frame.maxY >= window.bounds.maxY else { return nil }
        let keyboard = window.bounds.intersection(frame)
        return keyboard.isNull || keyboard.height <= window.safeAreaInsets.bottom + 0.5 ? nil : keyboard
    }
    private var focusedFieldBottom: CGFloat? {
        guard let window = factsWindow, let field = FocusedResponder.find() as? UIView, field.window === window else { return nil }
        return field.convert(field.bounds, to: window).maxY
    }
    private func measureWorkspaceBottom() {
        guard let window = factsWindow, window.bounds.height > 0 else { return }
        let keyboardInset = keyboardFrame?.height ?? 0
        if store.keyboardInset != keyboardInset {
            DispatchQueue.main.async { [weak store = store] in
                if store?.keyboardInset != keyboardInset { store?.keyboardInset = keyboardInset }
            }
        }
        let inset = keyboardCoversField ? keyboardInset : 0
        guard inset != workspaceBottom else { return }
        workspaceBottom = inset
        store.dispatch(["type": "measure_workspace_bottom", "inset": Double(inset)])
    }
    func stop() {
        frames.deactivate()
        endKeyRepeat()
        if attached { store.input(["type": "blur"]) }
        displayLink?.invalidate(); displayLink = nil
        if attached { store.native?.detach(); attached = false }
        shaderActivity.view?.removeGestureRecognizer(shaderActivity)
        calibrationHover.view?.removeGestureRecognizer(calibrationHover)
        contacts.removeAll(); ignoredContacts.removeAll(); sensorContacts.removeAll()
    }
    func setSceneActive(_ active: Bool) {
        guard sceneActive != active else { return }
        sceneActive = active
        capy_session_set_background(UIApplication.shared.applicationState != .active)
        if !active { endKeyRepeat() }
        wake()
    }
    func wake() { frames.wake() }
    private func updateAccessibility() {
        accessibilityLabel = store.bootstrap["drawing_canvas"].string
        accessibilityHint = store.bootstrap["drawing_canvas_help"].string
        accessibilityLanguage = store.interfaceLanguage.isEmpty ? nil : store.interfaceLanguage
        accessibilityValue = store.bootstrap[store.restartingCanvas ? "restarting_canvas" : store.canvasReady ? "canvas_ready" : "starting_canvas"].string
    }
    private func updateHeadroom() {
        let value = Double(window?.screen.currentEDRHeadroom ?? 1).clampedHeadroom
        if store.displayHeadroom != value {
            store.displayHeadroom = value; store.native?.displayHeadroom(value)
        }
        guard let screen = window?.screen else { return }
        let report = ScreenReport(wide: traitCollection.displayGamut == .P3, headroom: Double(screen.potentialEDRHeadroom))
        if report != screenReport { screenReport = report; store.native?.screenReport(report) }
    }
    @objc private func tick(_ link: CADisplayLink) {
        updateHeadroom()
        frames.tick(target: link.targetTimestamp)
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        becomeFirstResponder()
        route(touches, event: event, phase: 1)
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 2) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 3) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 4) }
    override func touchesEstimatedPropertiesUpdated(_ touches: Set<UITouch>) { updateEstimates(touches) }
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        store.native?.shaderInput()
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

enum FocusedResponder {
    fileprivate static weak var current: UIResponder?
    static func find() -> UIResponder? {
        current = nil
        UIApplication.shared.sendAction(#selector(UIResponder.captureFocusedResponder), to: nil, from: nil, for: nil)
        return current
    }
}
private extension UIResponder {
    @objc func captureFocusedResponder() { FocusedResponder.current = self }
}

private final class ShaderActivityRecognizer: UIGestureRecognizer, UIGestureRecognizerDelegate {
    var activity: (() -> Void)?
    var samples: ((Set<UITouch>, UIEvent, Double) -> Void)?
    private var active = Set<ObjectIdentifier>()
    init() {
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false; delaysTouchesBegan = false; delaysTouchesEnded = false
        delegate = self
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        active.formUnion(touches.map(ObjectIdentifier.init)); activity?(); samples?(touches, event, 1)
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) { activity?(); samples?(touches, event, 2) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        samples?(touches, event, 3); active.subtract(touches.map(ObjectIdentifier.init)); if active.isEmpty { state = .failed }
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        samples?(touches, event, 4); active.subtract(touches.map(ObjectIdentifier.init)); if active.isEmpty { state = .failed }
    }
    override func reset() { super.reset(); active.removeAll() }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
}
