import AppKit
import SwiftUI

/// A local event monitor exists only while this capture sheet is mounted. It
/// receives chords before menu equivalents, including Escape and Command keys.
struct ShortcutKeyCapture: NSViewRepresentable {
    let captured: (String, Bool, Bool, Bool) -> Void
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        let coordinator = context.coordinator
        coordinator.captured = captured
        coordinator.monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak view, weak coordinator] event in
            guard let view, event.window === view.window, view.window?.isKeyWindow == true else { return event }
            if !event.isARepeat {
                let key = AppleKeyName.name(event)
                let flags = event.modifierFlags
                if !key.isEmpty { coordinator?.captured?(key, !flags.intersection([.command, .control]).isEmpty,
                    flags.contains(.shift), flags.contains(.option)) }
            }
            return nil
        }
        return view
    }
    func updateNSView(_ view: NSView, context: Context) { context.coordinator.captured = captured }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor) }
        coordinator.monitor = nil; coordinator.captured = nil
    }
    final class Coordinator {
        var monitor: Any?
        var captured: ((String, Bool, Bool, Bool) -> Void)?
    }
}

enum AppleKeyName {
    static func name(_ event: NSEvent) -> String {
        let names: [UInt16: String] = [36: "enter", 48: "tab", 51: "backspace", 53: "escape",
            76: "enter", 115: "home", 116: "pageup", 117: "delete", 119: "end", 121: "pagedown",
            123: "arrowleft", 124: "arrowright", 125: "arrowdown", 126: "arrowup",
            122: "f1", 120: "f2", 99: "f3", 118: "f4", 96: "f5", 97: "f6", 98: "f7",
            100: "f8", 101: "f9", 109: "f10", 103: "f11", 111: "f12", 105: "f13",
            107: "f14", 113: "f15", 106: "f16", 64: "f17", 79: "f18", 80: "f19", 90: "f20"]
        let text = event.charactersIgnoringModifiers ?? ""
        if let scalar = text.unicodeScalars.first, text.unicodeScalars.count == 1,
            (0xF704...0xF71B).contains(scalar.value) { return "f\(scalar.value - 0xF703)" }
        return names[event.keyCode] ?? text
    }
}
