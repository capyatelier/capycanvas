import SwiftUI
import UIKit

@MainActor private final class SheetState: ObservableObject {
    @Published var sizeClass: UserInterfaceSizeClass = .regular
    var constructions = 0
    var mounts = 0
}

private struct ContentMount: UIViewRepresentable {
    let state: SheetState
    final class View: UIView {
        let state: SheetState
        init(state: SheetState) { self.state = state; super.init(frame: .zero) }
        required init?(coder: NSCoder) { fatalError() }
        override func didMoveToWindow() {
            super.didMoveToWindow()
            if window != nil { state.mounts += 1 }
        }
    }
    func makeUIView(context: Context) -> View {
        state.constructions += 1
        return View(state: state)
    }
    func updateUIView(_ view: View, context: Context) {}
}

private struct FittedContent: SwiftUI.View {
    @ObservedObject var state: SheetState
    var body: some SwiftUI.View {
        Color.clear.frame(width: 520, height: 360)
            .background(ContentMount(state: state))
            .modifier(EditorLegacyFittedPresentation())
            .environment(\.horizontalSizeClass, state.sizeClass)
    }
}

@MainActor private final class DisplayFrame: NSObject {
    private var continuation: CheckedContinuation<Void, Never>?
    private var link: CADisplayLink?
    func wait() async {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            let link = CADisplayLink(target: self, selector: #selector(frame))
            self.link = link
            link.add(to: .main, forMode: .common)
        }
    }
    @objc private func frame() {
        link?.invalidate(); link = nil
        continuation?.resume(); continuation = nil
    }
}

@main final class FittedSheetChecks: UIResponder, UIApplicationDelegate, UIWindowSceneDelegate {
    var window: UIWindow?

    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Reorder lifecycle", sessionRole: session.role)
        configuration.delegateClass = FittedSheetChecks.self
        return configuration
    }

    @MainActor private func settle(_ window: UIWindow) async {
        for _ in 0..<4 {
            window.layoutIfNeeded()
            await DisplayFrame().wait()
        }
    }
    @MainActor private func sizingControllers(_ controller: UIViewController) -> [UIViewController] {
        let matched = String(reflecting: type(of: controller)).contains("EditorFittedSheet.Controller") ? [controller] : []
        return matched + controller.children.flatMap { sizingControllers($0) }
    }
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        Task { @MainActor in
            let window = UIWindow(windowScene: scene)
            self.window = window
            let presenter = UIViewController()
            presenter.traitOverrides.horizontalSizeClass = .regular
            window.rootViewController = presenter
            window.makeKeyAndVisible()
            await settle(window)
            let state = SheetState()
            let sheet = UIHostingController(rootView: FittedContent(state: state))
            sheet.modalPresentationStyle = .pageSheet
            presenter.present(sheet, animated: false)
            await settle(window)
            let controllers = sizingControllers(sheet)
            precondition(controllers.count == 1, "The sheet must mount one sizing controller")
            let sizing = controllers[0]
            precondition(state.constructions == 1 && state.mounts == 1, "The sheet must mount its content once")
            precondition(abs(sheet.preferredContentSize.height - 360) < 1, "Regular sheets must use the content's ideal height")
            for sizeClass: UserInterfaceSizeClass in [.compact, .regular] {
                state.sizeClass = sizeClass
                await settle(window)
                precondition(presenter.traitCollection.horizontalSizeClass == .regular, "The presenting size class must remain regular")
                precondition(state.constructions == 1 && state.mounts == 1, "Changing child size class must preserve mounted sheet content")
                let current = sizingControllers(sheet)
                precondition(current.count == 1 && current[0] === sizing, "Changing child size class must preserve the sizing controller")
                precondition(abs(sheet.preferredContentSize.height - 360) < 1, "Child size class must not change the presenting sheet's ideal height")
            }
            for sizeClass: UIUserInterfaceSizeClass in [.compact, .regular] {
                presenter.traitOverrides.horizontalSizeClass = sizeClass
                await settle(window)
                precondition(state.sizeClass == .regular, "The child size class must remain regular")
                precondition(state.constructions == 1 && state.mounts == 1, "Changing presenter traits must preserve mounted sheet content")
                let current = sizingControllers(sheet)
                precondition(current.count == 1 && current[0] === sizing, "Changing presenter traits must preserve the sizing controller")
                precondition(sheet.sheetPresentationController?.prefersPageSizing == (sizeClass == .compact), "Native page sizing must follow the presenter's size class")
                if sizeClass == .regular {
                    precondition(abs(sheet.preferredContentSize.height - 360) < 1, "Returning to regular presentation must restore the ideal height")
                }
            }
            print("PASS: fitted sheet sizing: stable content and sizing ownership across child size classes")
            fflush(stdout)
            exit(0)
        }
    }
}
