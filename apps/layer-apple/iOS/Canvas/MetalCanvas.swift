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
    override class var layerClass: AnyClass { CAMetalLayer.self }
    let store: EditorStore
    private var displayLink: CADisplayLink?
    private var framePending = false
    private var generation: UInt64 = 0
    private var attached = false
    private var drawableExtent = CGSize.zero
    var contacts: [ObjectIdentifier: PencilContact] = [:]
    var nextContact: UInt64 = 0
    var ignoredContacts: Set<ObjectIdentifier> = []

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
        store.wake = { [weak self] in self?.wake() }
    }
    required init?(coder: NSCoder) { fatalError("Use init(store:)") }
    override var canBecomeFirstResponder: Bool { true }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if let window {
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
        } else { stop() }
    }
    override func layoutSubviews() {
        super.layoutSubviews()
        guard window != nil, bounds.width > 0, bounds.height > 0 else { return }
        let extent = CGSize(width: (bounds.width * contentScaleFactor).rounded(), height: (bounds.height * contentScaleFactor).rounded())
        guard extent != drawableExtent || !attached else { return }
        drawableExtent = extent
        let metal = layer as! CAMetalLayer
        metal.contentsScale = contentScaleFactor
        metal.drawableSize = extent
        let width = UInt32(extent.width), height = UInt32(extent.height)
        if !attached {
            store.native?.attach(metal, width: width, height: height, scale: Float(contentScaleFactor))
            attached = true
        } else { store.native?.resize(width: width, height: height, scale: Float(contentScaleFactor)) }
        wake()
    }
    func stop() {
        displayLink?.invalidate(); displayLink = nil
        if attached { store.native?.detach(); attached = false }
        contacts.removeAll(); ignoredContacts.removeAll()
    }
    func wake() { generation &+= 1; displayLink?.isPaused = false }
    @objc private func tick(_ link: CADisplayLink) {
        guard !framePending, attached, let native = store.native else { return }
        framePending = true
        let submittedGeneration = generation
        let now = UInt64(CACurrentMediaTime() * 1_000_000_000)
        let target = UInt64(link.targetTimestamp * 1_000_000_000)
        native.frame(now: now, target: target) { [weak self] again, revision, costs in
            DispatchQueue.main.async {
                guard let self else { return }
                self.framePending = false
                self.store.cameraRevision = revision
                if costs[2] > 0 { self.accessibilityValue = "Metal ready" }
                self.displayLink?.isPaused = !again && self.generation == submittedGeneration
            }
        }
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 1) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 2) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 3) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { route(touches, event: event, phase: 4) }
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        routeKeys(presses, pressed: true)
        super.pressesBegan(presses, with: event)
    }
    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        routeKeys(presses, pressed: false)
        super.pressesEnded(presses, with: event)
    }
}
