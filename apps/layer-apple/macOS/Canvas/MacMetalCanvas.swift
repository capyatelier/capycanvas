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
    private var pending = false
    private var generation: UInt64 = 0
    private var extent = CGSize.zero
    override var isFlipped: Bool { true }
    init(store: EditorStore) {
        self.store = store
        super.init(frame: .zero)
        wantsLayer = true
        let metal = CAMetalLayer()
        metal.isOpaque = true
        metal.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
        layer = metal
        store.wake = { [weak self] in self?.wake() }
    }
    required init?(coder: NSCoder) { fatalError("Use init(store:)") }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil {
            if displayLink == nil {
                let link = displayLink(target: self, selector: #selector(tick(_:)))
                link.add(to: .main, forMode: .common)
                displayLink = link
            }
            needsLayout = true
        } else { stop() }
    }
    override func layout() {
        super.layout()
        guard let window, let layer = layer as? CAMetalLayer, bounds.width > 0, bounds.height > 0 else { return }
        let scale = window.backingScaleFactor
        let next = CGSize(width: (bounds.width * scale).rounded(), height: (bounds.height * scale).rounded())
        guard next != extent || !attached else { return }
        extent = next; layer.contentsScale = scale; layer.drawableSize = next
        if !attached { store.native?.attach(layer, width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale)); attached = true }
        else { store.native?.resize(width: UInt32(next.width), height: UInt32(next.height), scale: Float(scale)) }
        wake()
    }
    func wake() { generation &+= 1; displayLink?.isPaused = false }
    func stop() { displayLink?.invalidate(); displayLink = nil; if attached { store.native?.detach(); attached = false } }
    @objc private func tick(_ link: CADisplayLink) {
        guard !pending, attached, let native = store.native else { return }
        pending = true
        let currentGeneration = generation
        native.frame(now: UInt64(CACurrentMediaTime() * 1_000_000_000), target: UInt64(link.targetTimestamp * 1_000_000_000)) { [weak self] again, revision, _ in
            DispatchQueue.main.async {
                guard let self else { return }
                self.pending = false; self.store.cameraRevision = revision
                self.displayLink?.isPaused = !again && currentGeneration == self.generation
            }
        }
    }
}
