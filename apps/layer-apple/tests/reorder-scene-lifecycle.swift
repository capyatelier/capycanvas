// Real UIKit windows with supplied lifecycle notifications and contact callbacks.
// This checks adapter isolation, not physical Pencil or OS interruption delivery.
import UIKit

@main final class ReorderSceneChecks: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Reorder lifecycle", sessionRole: session.role)
        configuration.delegateClass = ReorderSceneDelegate.self
        return configuration
    }
}

@MainActor final class ReorderSceneDelegate: UIResponder, UIWindowSceneDelegate {
    static var fixtures: [Fixture] = []
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let fixture = Fixture(scene)
        window = fixture.window
        Self.fixtures.append(fixture)
        window?.makeKeyAndVisible()
        if Self.fixtures.count == 1 {
            UIApplication.shared.requestSceneSessionActivation(nil, userActivity: nil, options: nil) { error in
                print("FAIL: second scene: \(error)"); fflush(stdout); exit(1)
            }
        } else if Self.fixtures.count == 2 {
            Task { @MainActor in
                // Let UIKit finish the initial scene activation before supplying
                // the lifecycle events whose scope this fixture checks.
                do {
                    try await Task.sleep(for: .milliseconds(300))
                    try Self.check(); print("PASS: native reorder scenes")
                }
                catch { print("FAIL: \(error)"); fflush(stdout); exit(1) }
                fflush(stdout); exit(0)
            }
        }
    }

    struct Failure: Error, CustomStringConvertible { let description: String }
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw Failure(description: message) }
    }
    enum Stage: CaseIterable { case pending, held, dragging, releasedMenu }

    @MainActor final class Fixture {
        let window: UIWindow
        let input = ReorderInputView(frame: CGRect(x: 0, y: 0, width: 300, height: 200))
        let model = WorkspaceRowInteraction()
        var commits = 0
        let origin = CGPoint(x: 80, y: 80)
        let destination = CGPoint(x: 80, y: 10)
        init(_ scene: UIWindowScene) {
            window = UIWindow(windowScene: scene)
            let controller = UIViewController()
            controller.view.backgroundColor = .white
            window.rootViewController = controller
            model.items = [JSON(["id": "first"]), JSON(["id": "second"])]
            for (index, id) in ["first", "second"].enumerated() {
                model.frames[id] = WorkspaceRowFrame(row: CGRect(x: 0, y: index * 56, width: 300, height: 56),
                    grip: CGRect(x: 0, y: index * 56, width: 20, height: 56))
            }
            model.commit = { [weak self] _, _ in self?.commits += 1 }
            input.model = model
            controller.view.addSubview(input)
        }
        func arm(_ device: ReorderDevice, _ stage: Stage) throws {
            model.cancel(); commits = 0; input.validate()
            guard let target = model.source(at: origin) else { throw Failure(description: "Missing row") }
            model.contact.prepare(target, device: device, origin: origin)
            if stage != .pending { model.recognizeHold() }
            if stage == .dragging {
                try require(input.moveReorder(destination) && model.drag != nil, "Contact must begin dragging")
            } else if stage == .releasedMenu {
                model.contact.release(at: origin)
                try require(model.menu != nil, "A touch/pen held release must retain its menu")
            }
        }
        var state: String {
            "\(model.contact.target?.id ?? "nil")/\(model.contact.held)/\(model.contact.dragging)/\(model.menu ?? "nil")/\(model.drag != nil)/\(commits)"
        }
        var cancelled: Bool {
            model.contact.target == nil && model.menu == nil && model.drag == nil && commits == 0
        }
    }

    static func deactivate(_ fixture: Fixture) {
        // UIKit posts both notifications when a scene resigns active; only the
        // scene notification identifies which editor is losing activity.
        NotificationCenter.default.post(name: UIScene.willDeactivateNotification, object: fixture.window.windowScene!)
        NotificationCenter.default.post(name: UIApplication.willResignActiveNotification, object: UIApplication.shared)
    }
    static func check() throws {
        let a = fixtures[0], b = fixtures[1]
        try require(a.window.windowScene !== b.window.windowScene, "The fixture needs two distinct UIKit scenes")
        var cases = 0
        for (owner, other) in [(a, b), (b, a)] {
            for device in [ReorderDevice.touch, .pen, .mouse] {
                for stage in Stage.allCases where stage != .releasedMenu || device != .mouse {
                    try owner.arm(device, stage); try other.arm(device, stage)
                    let retained = other.state
                    deactivate(owner)
                    try require(owner.cancelled, "Deactivated scene must cancel \(device) \(stage)")
                    try require(other.state == retained, "Another scene must retain \(device) \(stage)")
                    owner.model.contact.release(at: owner.destination)
                    try require(owner.cancelled, "A late release must not commit the cancelled contact")
                    other.model.contact.release(at: other.destination)
                    try require(other.commits == (stage == .dragging ? 1 : 0), "Unaffected scene must retain its drop")
                    try owner.arm(device, .dragging)
                    owner.model.contact.release(at: owner.destination)
                    try require(owner.commits == 1, "A new contact must work after cancellation")
                    cases += 1
                }
            }
        }
        // A marker can be reparented without being destroyed. Its current scene
        // must own cancellation, with no stale observer of the previous scene.
        a.input.removeFromSuperview()
        b.window.rootViewController!.view.addSubview(a.input)
        try a.arm(.pen, .dragging)
        let retained = a.state
        deactivate(a)
        try require(a.state == retained, "Previous scene must not cancel a reparented input")
        deactivate(b)
        try require(a.cancelled, "Current scene must cancel a reparented input")
        a.input.removeFromSuperview()
        print("PASS: \(cases) scene contact/menu cases, late release, resumed drops and reparented input")
    }
}
