import UIKit

@MainActor enum HomeScreenIcon {
    private static var wanted: String?
    private static var switching = false

    static func show(_ icon: String) {
        let alternates = (Bundle.main.object(forInfoDictionaryKey: "CFBundleIcons") as? [String: Any])?["CFBundleAlternateIcons"] as? [String: Any]
        let alternate = "AppIcon-\(icon)"
        wanted = alternates?[alternate] == nil ? nil : alternate
        switchIcon()
    }

    private static func switchIcon() {
        let application = UIApplication.shared
        guard !switching, application.supportsAlternateIcons, application.alternateIconName != wanted else { return }
        let target = wanted
        switching = true
        application.setAlternateIconName(target) { error in
            if let error { NSLog("Capy Canvas could not change the home screen icon: %@", error.localizedDescription) }
            Task { @MainActor in
                switching = false
                if wanted != target { switchIcon() }
            }
        }
    }
}
