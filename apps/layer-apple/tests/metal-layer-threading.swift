import Foundation
import QuartzCore
import Metal
import os
#if os(iOS)
import UIKit
#else
import AppKit
#endif

private struct Failure: Error { let message: String }
private final class NonViewDelegate: NSObject, CALayerDelegate {}

private final class CheckedMetalLayer: CAMetalLayer, @unchecked Sendable {
    let configurations = OSAllocatedUnfairLock(initialState: [(main: Bool, disabled: Bool)]())
    override var allowsNextDrawableTimeout: Bool {
        get { super.allowsNextDrawableTimeout }
        set {
            configurations.withLock { $0.append((Thread.isMainThread, CATransaction.disableActions())) }
            super.allowsNextDrawableTimeout = newValue
        }
    }
}

#if os(iOS)
private final class CheckedView: UIView {
    override class var layerClass: AnyClass { CheckedMetalLayer.self }
}
#else
private final class CheckedView: NSView {
    override func makeBackingLayer() -> CALayer { CheckedMetalLayer() }
}
#endif

private enum Checks {
    static func exercise(_ layer: CheckedMetalLayer, main: Bool, platform: UInt32) async throws {
        layer.configurations.withLock { $0.removeAll() }
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            let owner = Thread {
                do {
                    var bootstrap: UInt64 = 0
                    var error: UInt64 = 0
                    let prepared = #"{"saved":"","preferred_languages":["en"]}"#.withCString { capy_apple_prepare_launch($0, &error) }
                    defer { capy_apple_launch_free(prepared) }
                    let app = capy_apple_launch(platform, prepared, &bootstrap, &error)
                    _ = takeNativeText(bootstrap)
                    let launchError = takeNativeText(error)
                    guard app != 0 else { throw Failure(message: launchError ?? "Launch failed") }
                    defer { capy_apple_detach(app); capy_apple_destroy(app) }
                    func check(_ result: Int32) throws {
                        if result < 0 { throw Failure(message: takeNativeText(capy_apple_error(app)) ?? "Metal operation failed") }
                    }
                    let pointer = Unmanaged.passUnretained(layer).toOpaque()
                    try check(capy_apple_attach(app, pointer, 256, 256, 1))
                    try check(capy_apple_detach(app))
                    try check(capy_apple_attach(app, pointer, 320, 240, 1))
                    done.resume()
                } catch { done.resume(throwing: error) }
            }
            owner.name = "art.capycanvas.tests.layer-owner"
            owner.stackSize = 8 * 1024 * 1024
            owner.start()
        }
        let samples = layer.configurations.withLock { $0 }
        guard samples.count >= 2, samples.allSatisfy({ $0.main == main && $0.disabled }) else {
            throw Failure(message: "Expected at least two configurations on main=\(main) with actions disabled; got \(samples)")
        }
        print("PASS: Metal attach and resized reattach main=\(main), configurations=\(samples.count)")
    }
    @MainActor static func run() async throws {
        let view = CheckedView(frame: CGRect(x: 0, y: 0, width: 256, height: 256))
        #if os(iOS)
        let layer = view.layer as! CheckedMetalLayer
        let platform: UInt32 = 0
        #else
        view.wantsLayer = true
        let layer = view.layer as! CheckedMetalLayer
        let platform: UInt32 = 1
        #endif
        let delegate = NonViewDelegate()
        defer { withExtendedLifetime((view, delegate)) {} }
        guard layer.delegate === view else { throw Failure(message: "Fixture must use an actual view-backed layer") }
        try await exercise(layer, main: true, platform: platform)
        let child = CheckedMetalLayer()
        layer.addSublayer(child)
        try await exercise(child, main: true, platform: platform)
        let standalone = CheckedMetalLayer()
        try await exercise(standalone, main: false, platform: platform)
        standalone.delegate = delegate
        try await exercise(standalone, main: false, platform: platform)
        print("PASS: view-backed configuration uses main; standalone configuration stays on the owner")
    }
    @MainActor static func finish() async {
        do { try await run(); exit(0) }
        catch { print("FAIL: \(error)"); exit(1) }
    }
}

#if os(iOS)
private final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        Task { @MainActor in await Checks.finish() }
        return true
    }
}
@main private enum Main {
    static func main() { UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(AppDelegate.self)) }
}
#else
@main private enum Main {
    @MainActor static func main() async { await Checks.finish() }
}
#endif
