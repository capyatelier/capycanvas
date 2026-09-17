import AppKit

/// Disposable native source for cross-application drag acceptance.
private final class PhotoDragSource: NSView, NSDraggingSource {
    let url: URL
    var starts = 0
    init(_ url: URL) {
        self.url = url; super.init(frame: CGRect(x: 0, y: 0, width: 160, height: 44))
        setAccessibilityElement(true); setAccessibilityRole(.image)
        setAccessibilityIdentifier("photo-drag-source"); setAccessibilityLabel("Drag test photo")
        setAccessibilityValue("0")
    }
    required init?(coder: NSCoder) { fatalError("Programmatic test view") }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill(); bounds.fill()
        ("Drag test photo" as NSString).draw(at: CGPoint(x: 15, y: 15), withAttributes: [.foregroundColor: NSColor.labelColor])
    }
    override func mouseDown(with event: NSEvent) {
        starts += 1; setAccessibilityValue(String(starts))
        let item = NSDraggingItem(pasteboardWriter: url as NSURL)
        item.setDraggingFrame(bounds, contents: NSWorkspace.shared.icon(forFile: url.path))
        beginDraggingSession(with: [item], event: event, source: self)
    }
    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation { .copy }
}

@main final class PhotoDragApplication: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    static func main() {
        let app = NSApplication.shared
        let delegate = PhotoDragApplication(); app.delegate = delegate
        app.setActivationPolicy(.regular); app.run()
        withExtendedLifetime(delegate) {}
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        let args = CommandLine.arguments
        let source = PhotoDragSource(URL(fileURLWithPath: args[1]))
        let window = NSWindow(contentRect: source.bounds, styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.level = .floating; window.title = "Photo drop fixture"
        window.contentView = source
        window.setFrameTopLeftPoint(CGPoint(x: Double(args[2])!, y: NSScreen.screens[0].frame.maxY - Double(args[3])!))
        self.window = window; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }
}
