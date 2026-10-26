import AppKit
import SwiftUI

struct ShortcutKeyCapture: NSViewRepresentable {
    let captured: (String, Bool, Bool, Bool) -> Bool
    private let selective: Bool
    init(captured: @escaping (String, Bool, Bool, Bool) -> Void) {
        self.captured = { key, command, shift, alt in captured(key, command, shift, alt); return true }
        selective = false
    }
    init(consuming: @escaping (String, Bool, Bool, Bool) -> Bool) {
        captured = consuming; selective = true
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        let coordinator = context.coordinator
        coordinator.captured = captured
        coordinator.selective = selective
        coordinator.monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { [weak view, weak coordinator] event in
            guard let view, event.window === view.window, view.window?.isKeyWindow == true else { return event }
            guard (view.window?.firstResponder as? NSTextInputClient)?.hasMarkedText() != true else { return event }
            if event.type == .flagsChanged {
                if coordinator?.selective == true { return event }
                let flags = event.modifierFlags, previous = coordinator?.flags ?? []
                coordinator?.flags = flags
                for (flag, name): (NSEvent.ModifierFlags, String) in [(.shift, "Shift"), (.control, "Control"), (.option, "Alt"), (.command, "Meta")]
                    where flags.contains(flag) && !previous.contains(flag) {
                    _ = coordinator?.captured?(name, !flags.intersection([.command, .control]).isEmpty, flags.contains(.shift), flags.contains(.option))
                }
            } else if !event.isARepeat {
                let key = AppleKeyName.name(event)
                let flags = event.modifierFlags
                if !key.isEmpty {
                    let consumed = coordinator?.captured?(key, !flags.intersection([.command, .control]).isEmpty,
                        flags.contains(.shift), flags.contains(.option)) == true
                    if coordinator?.selective == true { return consumed ? nil : event }
                }
            }
            return coordinator?.selective == true ? event : nil
        }
        return view
    }
    func updateNSView(_ view: NSView, context: Context) {
        context.coordinator.captured = captured; context.coordinator.selective = selective
    }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor) }
        coordinator.monitor = nil; coordinator.captured = nil
    }
    final class Coordinator {
        var monitor: Any?
        var selective = false
        var flags: NSEvent.ModifierFlags = []
        var captured: ((String, Bool, Bool, Bool) -> Bool)?
    }
}

enum AppleKeyName {
    static func name(_ event: NSEvent) -> String {
        let names: [UInt16: String] = [36: "enter", 48: "tab", 51: "backspace", 53: "escape",
            76: "enter", 114: "insert", 115: "home", 116: "pageup", 117: "delete", 119: "end", 121: "pagedown",
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
