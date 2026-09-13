import UIKit

extension AppleContextMenu {
    func nativeMenu() -> UIMenu { Self.menu(title: title, sections: sections) }
    private static func menu(title: String, sections: [[Item]]) -> UIMenu {
        UIMenu(title: title, children: sections.map { section in
            UIMenu(options: .displayInline, children: section.map { entry -> UIMenuElement in
                // UIKit submenus have no disabled attribute. Keep a disabled
                // branch visible as an unavailable item; never enable its
                // descendants merely to present the hierarchy.
                if entry.enabled && !entry.sections.isEmpty { return menu(title: entry.label, sections: entry.sections) }
                let action = UIAction(title: entry.label,
                    discoverabilityTitle: entry.hint.isEmpty ? nil : entry.hint,
                    attributes: entry.enabled ? [] : .disabled,
                    state: entry.selected == true ? .on : .off) { _ in entry.action?() }
                if !entry.hint.isEmpty { action.subtitle = entry.hint }
                return action
            })
        })
    }
}
