// Fast targeted editor check, independent of XCTest automation startup. Launches
// its own disposable editor; never addresses the system menu bar or other apps.
import AppKit
import ApplicationServices

enum ProbeError: Error { case failed(String) }
func require(_ value: Bool, _ message: String) throws { if !value { throw ProbeError.failed(message) } }
func pump() { RunLoop.current.run(until: Date().addingTimeInterval(0.04)) }
func wait(_ description: String, _ predicate: () -> Bool) throws {
    let deadline = Date().addingTimeInterval(20)
    while !predicate() { try require(Date() < deadline, "Timed out: " + description); pump() }
}
func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success ? value : nil
}
func find(_ root: AXUIElement, _ identifier: String) -> AXUIElement? {
    if attribute(root, kAXIdentifierAttribute) as? String == identifier { return root }
    for child in attribute(root, kAXChildrenAttribute) as? [AXUIElement] ?? [] {
        if let result = find(child, identifier) { return result }
    }
    return nil
}
func rect(_ element: AXUIElement) throws -> CGRect {
    guard let position = attribute(element, kAXPositionAttribute), let size = attribute(element, kAXSizeAttribute) else {
        throw ProbeError.failed("Missing control geometry")
    }
    var origin = CGPoint.zero, extent = CGSize.zero
    try require(AXValueGetValue(position as! AXValue, .cgPoint, &origin)
        && AXValueGetValue(size as! AXValue, .cgSize, &extent), "Invalid control geometry")
    return CGRect(origin: origin, size: extent)
}

do {
try require(CommandLine.arguments.count == 2, "Pass the built macOS .app path")
try require(AXIsProcessTrusted(), "Direct editor testing requires existing Accessibility permission")
let configuration = NSWorkspace.OpenConfiguration()
configuration.createsNewApplicationInstance = true
configuration.arguments = ["-ApplePersistenceIgnoreState", "YES"]
configuration.environment = ["CAPY_DISABLE_PERSISTENCE": "1", "CAPY_INITIAL_ACTIONS": #"[{"type":"set_theme","theme":"light"},{"type":"effect","action":{"op":"insert","effect":"gaussian_blur"}},{"type":"effect","action":{"op":"set","layer":3,"key":"sigma","value":{"kind":"number","value":5}}},{"type":"effect","action":{"op":"insert","effect":"curves"}}]"#]
var launched: NSRunningApplication?, launchError: Error?
NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: CommandLine.arguments[1]), configuration: configuration) { app, error in
    launched = app; launchError = error
}
try wait("isolated editor launch") { launched != nil || launchError != nil }
if let launchError { throw launchError }
let app = launched!, root = AXUIElementCreateApplication(app.processIdentifier)
func control(_ id: String) throws -> AXUIElement {
    try wait(id) { find(root, id) != nil }
    return find(root, id)!
}
func expect(_ id: String, _ value: String) throws {
    try wait(id + " = " + value) { find(root, id).flatMap { attribute($0, kAXValueAttribute) as? String } == value }
}
func press(_ id: String) throws {
    let element = try control(id)
    try require(AXUIElementPerformAction(element, kAXPressAction as CFString) == .success, "Press " + id)
}
func click(_ id: String, x: Double, y: Double) throws {
    let bounds = try rect(control(id))
    let point = CGPoint(x: bounds.minX + bounds.width * x, y: bounds.minY + bounds.height * y)
    for (type, button) in [(CGEventType.leftMouseDown, CGMouseButton.left), (.leftMouseUp, .left)] {
        CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: button)?.postToPid(app.processIdentifier)
        pump()
    }
}
func type(_ text: String, submit: Bool = false) {
    let units = Array(text.utf16)
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down)!
        event.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
        event.postToPid(app.processIdentifier); pump()
    }
    if submit {
        for down in [true, false] { CGEvent(keyboardEventSource: nil, virtualKey: 36, keyDown: down)?.postToPid(app.processIdentifier); pump() }
    }
}
try expect("canvas", "Metal ready")
try expect("effect-curve", "2 points")
try click("effect-curve", x: 0.5, y: 0.25); try expect("effect-curve", "3 points")
try press("curve-reset"); try expect("effect-curve", "2 points")
try press("panel-tab-adjustments"); try press("filter-search-toggle")
_ = try control("filter-search"); type("Gradient Map")
try expect("adjustment-gradient_map", "Preview ready")
try press("adjustment-gradient_map"); try expect("effect-gradient", "2 stops")
try click("effect-gradient", x: 0.5, y: 0.4); try expect("effect-gradient", "3 stops")
try click("effect-gradient", x: 0.5, y: 0.4)
try press("number-value-gradient-position"); _ = try control("number-entry-gradient-position")
type("25", submit: true); try expect("number-value-gradient-position", "25.0 %")
try press("gradient-reset"); try expect("effect-gradient", "2 stops")
print("PASS: Mac curve insertion/reset, filter search/GPU preview, gradient insertion/position/reset")
} catch {
    FileHandle.standardError.write(Data("FAIL: \(error)\n".utf8))
    exit(1)
}
