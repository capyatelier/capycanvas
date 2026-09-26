import AppKit
import SwiftUI

/// Native events in one owned fixture window; never global mouse coordinates.
class NativeWorkspaceInputFixture {
    @MainActor static var previousPoint: CGPoint?
    static func note(_ text: String) { FileHandle.standardError.write(Data((text + "\n").utf8)) }
    @MainActor static func drain(_ seconds: Double = 0.08) async throws {
        try await Task.sleep(for: .seconds(seconds))
    }
    @MainActor static func find(_ view: NSView) -> ReorderInputView? {
        if let marker = view as? ReorderInputView, marker.model != nil { return marker }
        return view.subviews.lazy.compactMap(find).first
    }
    @MainActor static func holdDuration(_ marker: ReorderInputView) throws -> TimeInterval {
        var container: NSView? = marker
        while let view = container {
            if let press = view.gestureRecognizers.first(where: { $0 is NSPressGestureRecognizer }) as? NSPressGestureRecognizer {
                return press.minimumPressDuration + 0.15
            }
            container = view.superview
        }
        throw HostFailure(message: "The fixture has no native hold recognizer")
    }
    @MainActor static func key(_ characters: String, code: UInt16, window: NSWindow) throws {
        for type: NSEvent.EventType in [.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code) else { throw HostFailure(message: "No native fixture key") }
            NSApp.postEvent(event, atStart: false)
        }
    }
    @MainActor static func event(_ type: NSEvent.EventType, at point: CGPoint, marker: NSView, number: Int, tablet: Bool = false) throws {
        guard let window = marker.window, let event = NSEvent.mouseEvent(with: type,
            location: marker.convert(point, to: nil), modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, eventNumber: number, clickCount: 1, pressure: 0.5) else {
            throw HostFailure(message: "No native fixture event")
        }
        // mouseEvent(with:) leaves buttonNumber at zero even for right-down/up.
        // Rewrap the CGEvent to deliver an actual secondary-button contact.
        guard let cg = event.cgEvent else { throw HostFailure(message: "Missing fixture CGEvent") }
        let button: Int64 = type == .rightMouseDown || type == .rightMouseUp ? 1 : 0
        cg.setIntegerValueField(.mouseEventButtonNumber, value: button)
        cg.setIntegerValueField(.mouseEventNumber, value: Int64(number))
        if tablet { cg.setIntegerValueField(.mouseEventSubtype, value: Int64(CGEventMouseSubtype.tabletPoint.rawValue)) }
        if type == .leftMouseDragged, let previousPoint {
            cg.setDoubleValueField(.mouseEventDeltaX, value: point.x - previousPoint.x)
            cg.setDoubleValueField(.mouseEventDeltaY, value: point.y - previousPoint.y)
        }
        previousPoint = point
        guard let delivered = NSEvent(cgEvent: cg) else { throw HostFailure(message: "Invalid fixture CGEvent") }
        // The CGEvent round trip can round fractional points by a few ULPs.
        let location = delivered.locationInWindow
        try require(delivered.type == type && delivered.buttonNumber == button
            && delivered.eventNumber == number && delivered.windowNumber == window.windowNumber
            && abs(location.x - event.locationInWindow.x) < 1e-6
            && abs(location.y - event.locationInWindow.y) < 1e-6,
            "Fixture event must retain the native button, window and measured location: type=\(delivered.type.rawValue)/\(type.rawValue), button=\(delivered.buttonNumber)/\(button), number=\(delivered.eventNumber)/\(number), window=\(delivered.windowNumber)/\(window.windowNumber), location=\(delivered.locationInWindow)/\(event.locationInWindow)")
        try require((delivered.subtype == .tabletPoint) == tablet, "The fixture must retain the actual tablet subtype")
        NSApp.postEvent(delivered, atStart: false)
    }
}
