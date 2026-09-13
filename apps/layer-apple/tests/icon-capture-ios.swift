import SwiftUI
import UIKit

/// Component evidence from UIKit's real hosting/rendering path. This does not
/// capture an editor, exercise input, or establish physical-device parity.
@MainActor private final class Delegate: NSObject, UIApplicationDelegate {
    var window: UIWindow?

    func application(_ application: UIApplication,
        didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        Task { @MainActor in
            do { try await capture(); print("PASS: UIKit shared icon captures"); exit(0) }
            catch { print("FAIL: UIKit icon capture: \(error)"); exit(1) }
        }
        return true
    }

    private func capture() async throws {
        guard let input = Bundle.main.url(forResource: "fixtures", withExtension: "json"),
            let paintsData = NSDataAsset(name: "shared-icon-paints")?.data else {
            throw HostFailure(message: "Missing fixture manifest or current compiled icon assets")
        }
        let manifest = try JSON.decode(String(contentsOf: input, encoding: .utf8))
        let paints = try JSON.decode(String(decoding: paintsData, as: UTF8.self))
        precondition(manifest["schema"].number == 1 && !manifest["fixtures"].array.isEmpty)
        let output = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("captures", isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        let container = UIViewController()
        let window = UIWindow(frame: UIScreen.main.bounds)
        self.window = window; window.rootViewController = container; window.makeKeyAndVisible()
        defer { window.isHidden = true; window.rootViewController = nil; self.window = nil }
        for fixture in manifest["fixtures"].array {
            let names = fixture["icons"].array.map(\.string)
            for name in names {
                let key = SharedIcon.assetKey(name)
                let layers = paints[key].array.map { $0["asset"].string }
                let assets = layers.isEmpty ? ["icon-" + key] : layers
                precondition(SharedIcon.assetKey(key) == key && assets.allSatisfy { UIImage(named: $0) != nil },
                    "Missing compiled shared icon: \(name)")
            }
            let width = fixture["width"].number, height = fixture["height"].number
            let dark = fixture["theme"].string == "dark"
            let content = ZStack(alignment: .topLeading) {
                ForEach(names.indices, id: \.self) { index in
                    SharedIcon(name: names[index], size: fixture["size"].number)
                        .foregroundStyle(Color(hex: fixture["foreground"].string))
                        .opacity(fixture["opacity"].number)
                        .position(x: CGFloat(index % 12 * 48 + 24), y: CGFloat(index / 12 * 48 + 24))
                }
            }.frame(width: width, height: height)
                .background(Color(hex: fixture["background"].string))
                .environment(\.colorScheme, dark ? .dark : .light)
                .environment(\.displayScale, fixture["scale"].number)
            let host = UIHostingController(rootView: content)
            host.safeAreaRegions = []; host.overrideUserInterfaceStyle = dark ? .dark : .light
            container.addChild(host); container.view.addSubview(host.view); host.didMove(toParent: container)
            host.view.frame = CGRect(x: 0, y: 0, width: width, height: height)
            for _ in 0..<10 { host.view.setNeedsLayout(); host.view.layoutIfNeeded(); try await Task.sleep(for: .milliseconds(10)) }
            let format = UIGraphicsImageRendererFormat()
            format.scale = fixture["scale"].number; format.opaque = true; format.preferredRange = .standard
            var painted = false
            let image = UIGraphicsImageRenderer(size: host.view.bounds.size, format: format).image { _ in
                painted = host.view.drawHierarchy(in: host.view.bounds, afterScreenUpdates: true)
            }
            guard painted, let png = image.pngData() else { throw HostFailure(message: "UIKit did not paint the icon grid") }
            try png.write(to: output.appendingPathComponent("native-\(fixture["name"].string).png"))
            host.willMove(toParent: nil); host.view.removeFromSuperview(); host.removeFromParent()
        }
        try FileManager.default.copyItem(at: input, to: output.appendingPathComponent("fixtures.json"))
    }
}

@main struct UIKitIconCaptures {
    static func main() { UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(Delegate.self)) }
}
