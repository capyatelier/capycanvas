import AppKit
import SwiftUI

extension AppleContextMenu {
    func nativeMenu() -> NSMenu { Self.menu(title: title, sections: sections) }
    private static func menu(title: String, sections: [[Item]]) -> NSMenu {
        let result = NSMenu(title: title); result.autoenablesItems = false
        for (sectionIndex, section) in sections.enumerated() {
            if sectionIndex > 0 { result.addItem(.separator()) }
            for entry in section {
                let item = NSMenuItem(title: entry.label, action: nil, keyEquivalent: "")
                item.isEnabled = entry.enabled; item.state = entry.selected == true ? .on : .off
                if !entry.hint.isEmpty { item.toolTip = entry.hint }
                if let binding = entry.bindings.first, let shortcut = menuShortcut(binding) {
                    item.keyEquivalent = String(shortcut.key.character)
                    var modifiers: NSEvent.ModifierFlags = []
                    if shortcut.modifiers.contains(.command) { modifiers.insert(.command) }
                    if shortcut.modifiers.contains(.control) { modifiers.insert(.control) }
                    if shortcut.modifiers.contains(.option) { modifiers.insert(.option) }
                    if shortcut.modifiers.contains(.shift) { modifiers.insert(.shift) }
                    item.keyEquivalentModifierMask = modifiers
                }
                if !entry.sections.isEmpty {
                    item.submenu = menu(title: entry.label, sections: entry.sections)
                } else if let action = entry.action {
                    let target = NativeMenuAction(action)
                    item.target = target; item.action = #selector(NativeMenuAction.invokeMenuItem(_:))
                    // NSMenuItem's target is weak. The item owns this callback
                    // for exactly the lifetime of the native menu item.
                    item.representedObject = target
                }
                result.addItem(item)
            }
        }
        return result
    }
}

@MainActor private final class NativeMenuAction: NSObject {
    let action: () -> Void
    init(_ action: @escaping () -> Void) { self.action = action }
    @objc func invokeMenuItem(_ sender: NSMenuItem) { if sender.isEnabled { action() } }
}
