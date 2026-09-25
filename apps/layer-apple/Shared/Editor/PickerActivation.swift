import SwiftUI
#if canImport(AppKit)
import AppKit
#endif

@MainActor enum PickerActivation {
    private static var last: (anchor: String, time: TimeInterval)?
    static func activate(_ control: JSON, anchor: [String: Any], store: EditorStore, normally: () -> Void) {
        let picker = control["kind"].string == "color_picker"
            || control["kind"].string == "command" && control["command"].string == "eyedropper"
        guard picker, pointerActivation else { last = nil; normally(); return }
        let key = JSON(anchor).stableKey, now = ProcessInfo.processInfo.systemUptime
        if let last, last.anchor == key, now - last.time <= interval {
            self.last = nil
            store.dispatch(["type": "color_picker", "action": ["kind": "settings", "anchor": anchor]])
        } else {
            last = (key, now)
            normally()
        }
    }
    private static var interval: TimeInterval {
        #if canImport(AppKit)
        NSEvent.doubleClickInterval
        #else
        0.35
        #endif
    }
    private static var pointerActivation: Bool {
        #if canImport(AppKit)
        guard let event = NSApp.currentEvent else { return false }
        return [.leftMouseUp, .leftMouseDown, .tabletPoint].contains(event.type)
        #else
        true
        #endif
    }
}
