import SwiftUI
import CoreText
import CryptoKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

@MainActor private final class Showcase {
    let root: URL
    let job: JSON
    let store: EditorStore
    init() async throws {
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("StoreCapture")
        let arguments = CommandLine.arguments
        let name = arguments.firstIndex(of: "--job").map { arguments[$0 + 1] } ?? "job.json"
        let url = name.hasPrefix("/") ? URL(fileURLWithPath: name) : documents.appendingPathComponent(name)
        root = name.hasPrefix("/") ? url.deletingLastPathComponent() : documents
        job = try await Task.detached { try JSON.decode(String(contentsOf: url, encoding: .utf8)) }.value
        #if os(macOS)
        let platform: UInt32 = 1
        #else
        let platform: UInt32 = 0
        #endif
        store = EditorStore(platform: platform, persistence: EditorPersistence(root: root.appendingPathComponent(job["run"].string + "/storage")), deferLaunch: true)
    }
    func write(_ value: JSON, _ name: String) async throws {
        try await navigationSave(value, to: root.appendingPathComponent(job["run"].string + "/" + name))
    }
    func run() async throws {
        try await wait("native workspace and presented canvas", seconds: 180, failure: { self.store.failure }) {
            self.store.workspaces?.ready == true && !self.store.launchVisible && self.store.canvasReady
        }
        let recipeURL = Bundle.main.url(forResource: "store-recipe", withExtension: "json")!
        let recipeBytes = try await Task.detached { try Data(contentsOf: recipeURL) }.value
        let recipe = try JSON.decode(String(decoding: recipeBytes, as: UTF8.self))
        let captionsURL = Bundle.main.url(forResource: "expected-scope-exact", withExtension: "json")!
        let captions = try await Task.detached { try JSON.decode(String(contentsOf: captionsURL, encoding: .utf8)) }.value
        let selected = recipe["scenes"].array.filter { $0["id"].string == job["scene"].string }
        try require(selected.count == 1, "Exactly one canonical scene")
        let scene = selected[0]
        let workspace = "builtin:workspace:" + scene["workspace"].string
        try await store.workspaces!.perform(["type": "switch", "id": workspace])
        let source = Bundle.main.resourceURL!.appendingPathComponent(scene["source"].string)
        let sourceHash = try await Task.detached { SHA256.hash(data: try Data(contentsOf: source)).map { String(format: "%02x", $0) }.joined() }.value
        try require(sourceHash == scene["sourceSha256"].string, "Canonical artwork SHA-256")
        store.projectFiles.openURL(source)
        try await wait("canonical artwork admission", seconds: 180, failure: { self.store.failure ?? self.store.projectFiles.error }) {
            !self.store.projectFiles.busy && self.store.state["tabs"].array.contains {
                $0["active"].bool && $0["width"].uint > 0 && $0["title"].string.hasPrefix(source.deletingPathExtension().lastPathComponent)
            }
        }
        try await ready()
        try await store.apply(["type": "invoke", "command": "fit_canvas"])
        var adjustments: [String: UInt64] = [:]
        func layer(_ name: String) throws -> UInt64 {
            let rows = store.state["layers"].array.filter { $0["label"].string == name }
            try require(rows.count == 1, "Unique authored layer: \(name)")
            return rows[0]["id"].uint
        }
        for step in scene["steps"].array {
            switch step["type"].string {
            case "action": try await store.apply(step["action"].object)
            case "select_layer": try await store.apply(["type": "layer", "action": ["op": "select", "id": try layer(step["name"].string), "mask": false]])
            case "layer_visibility": try await store.apply(["type": "layer", "action": ["op": "visibility", "id": try layer(step["name"].string), "value": step["visible"].bool]])
            case "show_panel":
                try await store.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": step["panel"].string, "visible": true]])
            case "effect":
                let effect = step["effect"].string
                try await store.apply(["type": "effect", "action": ["op": "insert", "effect": effect]])
                try await ready()
                let id = store.state["layer_properties"]["layer"].uint
                adjustments[effect] = id
                for (key, value) in step["parameters"].object {
                    try await store.apply(["type": "effect", "action": ["op": "set", "layer": id, "key": key, "value": value]])
                }
            case "frame":
                if job["device"].string != "iphone" { try await store.apply(["type": "set_zoom", "zoom": step["zoom"].number]) }
                let camera = store.state["camera"], area = camera["work_area"], focus = step["focus"]
                let tab = store.state["tabs"].array.first { $0["active"].bool }!
                let x = area[0].number + area[2].number / 2, y = area[1].number + area[3].number / 2
                let dx = (tab["width"].number / 2 - focus[0].number) * camera["zoom"].number
                let dy = (tab["height"].number / 2 - focus[1].number) * camera["zoom"].number
                for (phase, offset) in [("down", 0.0), ("move", 1.0), ("up", 1.0)] {
                    await withCheckedContinuation { done in
                        store.semanticInput(JSON(["type": "pointer", "kind": "mouse", "button": "pan", "id": 17,
                            "phase": phase, "position": [x + offset * dx, y + offset * dy], "time_ns": FrameTrace.now()])) { _ in done.resume() }
                    }
                }
            case "wait_histogram": try await ready()
            default: throw HostFailure(message: "Unsupported canonical scene step: \(step.stableKey)")
            }
        }
        for language in job["languages"].array.map(\.string) {
            let tags = store.bootstrap["shipped_tags"].array.map(\.string)
            guard let index = tags.firstIndex(of: language) else { throw HostFailure(message: "Unshipped language: \(language)") }
            try await store.apply(["type": "preferences", "action": ["type": "edit", "id": "language", "value": index + 1]])
            try await wait("localized native editor", failure: { self.store.failure }) { self.store.interfaceLanguage == language }
            for (effect, id) in adjustments {
                let label = store.state["adjustments"].array.first { $0["id"].string == effect }!["label"].string
                try await store.apply(["type": "layer", "action": ["op": "rename", "id": id, "name": label]])
            }
            for theme in job["themes"].array.map(\.string) {
                try await store.apply(["type": "set_theme", "theme": theme])
                try await ready()
                if job["device"].string == "iphone" {
                    if !store.state["customization"]["drawer"].isNull {
                        try await store.apply(["type": "customize", "action": ["type": "close_expanded"]])
                    }
                    let layers = store.state["workspace"]["layout"]["phone"].array.first {
                        $0["control"]["kind"].string == "panel" && $0["control"]["panel"].string == "layers"
                    }!
                    try await store.apply(["type": "phone", "action": ["kind": "tool", "tile": layers["id"].raw]])
                    try await wait("native Layers drawer", failure: { self.store.failure }) {
                        self.store.contentDrawers.items["tool"]?.interactive == true
                    }
                    if scene["id"].string == "photo" {
                        store.contentDrawers.items["tool"]!.toggleProperties()
                        try await wait("native Properties drawer page", failure: { self.store.failure }) {
                            self.store.contentDrawers.items["tool"]?.pagePosition == 1
                        }
                    }
                }
                let panels = store.snapshot["layout"]["groups"].array.map { $0["active"].string }
                var scopes = ["histogram", "waveform"].filter { panels.contains($0) }
                if store.state["layer_properties"]["histogram"].bool &&
                    (panels.contains("properties") || store.contentDrawers.items["tool"]?.propertiesOpen == true) { scopes.append("tonal_histogram") }
                try await wait("exact native scope plots", seconds: 180, failure: { self.store.failure }) {
                    scopes.allSatisfy { kind in
                        self.store.state[kind]["status"].string == captions[language].string &&
                            !self.store.state[kind]["data"].isNull && self.store.scopes.plots[kind].map { kind == "waveform" ? $0.image != nil : !$0.bins.isEmpty } == true
                    }
                }
                try await previews(theme)
                try await ready()
                let text = texts(store.snapshot.json)
                let fonts = try await Task.detached { try Self.fonts(text) }.value
                let token = "\(language)/\(scene["id"].string)-\(theme)"
                var report: [String: Any] = ["token": token, "device": job["device"].string, "language": language,
                    "scene": scene["id"].string, "theme": theme, "workspace": workspace, "source": scene["source"].string,
                    "source_sha256": sourceHash, "recipe_sha256": SHA256.hash(data: recipeBytes).map { String(format: "%02x", $0) }.joined(),
                    "canvas_ready": store.canvasReady, "brush_ready": store.snapshot["brush_ready"].bool,
                    "rendering_pending": false, "previews_ready": true, "exact_scopes": scopes,
                    "ui_text": text, "glyphs_checked": fonts.0, "missing_glyphs": fonts.1,
                    "snapshot": store.snapshot.json.raw, "capture_source": "native-controls-and-metal"]
                if job["device"].string == "iphone" {
                    report["native_adaptation"] = ["histogram": "The native phone Layers/Properties drawer does not expose the composite Histogram panel."]
                }
                #if os(macOS)
                guard let window = NSApp.keyWindow else { throw HostFailure(message: "Owned capture window unavailable") }
                report["window_number"] = window.windowNumber
                report["window_bounds"] = [window.frame.minX, NSScreen.screens[0].frame.height - window.frame.maxY, window.frame.width, window.frame.height]
                #elseif targetEnvironment(simulator)
                report["simulator"] = true
                #else
                report["simulator"] = false
                #endif
                try await write(JSON(report), "ready.json")
                let ack = root.appendingPathComponent(job["run"].string + "/ack.json")
                var acknowledged = false
                try await wait("external composed capture acknowledgement", seconds: 180, step: {
                    acknowledged = await Task.detached {
                        (try? String(contentsOf: ack, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines) == token
                    }.value
                }) { acknowledged }
            }
        }
        try await write(JSON(["complete": true]), "ready.json")
    }
    func ready() async throws {
        try await wait("canvas and brush admission", seconds: 180, failure: { self.store.failure }) {
            self.store.canvasReady && self.store.snapshot["brush_ready"].bool && !self.store.projectFiles.busy
        }
        try await prepare(store.native!, "showcase edits")
        var pending = true
        try await wait("native rendering settled", seconds: 180, failure: { self.store.failure }, step: {
            await frame(self.store.native!)
            pending = await withCheckedContinuation { done in
                self.store.native!.renderingPending { done.resume(returning: $0) }
            }
        }) { !pending }
    }
    func previews(_ theme: String) async throws {
        let active = store.snapshot["layout"]["groups"].array.map { $0["active"].string }
        let drawer = store.contentDrawers.items["tool"]
        let layersShown = active.contains("layers") || (drawer?.interactive == true && drawer?.propertiesOpen == false)
        try await wait("mounted layer previews", seconds: 120, failure: { self.store.failure }) {
            if !layersShown { return true }
            let rows = self.store.state["layers"].array
            if layersShown && rows.contains(where: { $0["has_thumbnail"].bool }) && self.store.layerThumbnails.visible.isEmpty { return false }
            return rows.filter { self.store.layerThumbnails.visible.values.contains($0["id"].uint) }.allSatisfy { row in
                [false, true].allSatisfy { mask in
                    !row[mask ? "has_mask" : "has_thumbnail"].bool || self.store.layerThumbnails.images[LayerThumbnails.key(row["id"].uint, mask)] != nil
                }
            }
        }
        var models = active.contains("brushes") ? [store.state["tool_set"]] : []
        models += active.compactMap { store.state["tool_panels"][$0].isNull ? nil : store.state["tool_panels"][$0] }
        let names = Set(models.flatMap { $0["subtools"].array }.filter { !$0["preview"].isNull }.map { "preview-\($0["preview"].uint)-\(theme)" })
        try await wait("mounted bundled brush previews", seconds: 120, failure: { self.store.failure }) {
            names.allSatisfy { BundledPreview.images.object(forKey: $0 as NSString) != nil }
        }
    }
    func texts(_ value: JSON) -> [String] {
        var result = Set<String>()
        func walk(_ value: JSON) {
            for (key, raw) in value.object {
                let child = JSON(raw)
                if ["label", "title", "caption", "status", "text"].contains(key), !child.string.isEmpty { result.insert(child.string) }
                walk(child)
            }
            for child in value.array { walk(child) }
        }
        walk(value)
        return result.sorted()
    }
    nonisolated static func fonts(_ texts: [String]) throws -> (Int, [String]) {
        var count = 0, missing: [String] = []
        let font = CTFontCreateUIFontForLanguage(.system, 13, nil)!
        for text in texts {
            try require(!text.contains("\u{FFFD}") && !text.contains("\u{25A1}"), "Replacement glyph in native UI: \(text)")
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font]))
            for run in CTLineGetGlyphRuns(line) as! [CTRun] {
                var glyphs = [CGGlyph](repeating: 0, count: CTRunGetGlyphCount(run))
                CTRunGetGlyphs(run, CFRange(location: 0, length: 0), &glyphs)
                count += glyphs.count
                if glyphs.contains(0) { missing.append(text) }
            }
        }
        try require(count > 0 && missing.isEmpty, "Native font coverage: \(missing)")
        return (count, missing)
    }
}

#if os(macOS)
@MainActor private final class CaptureDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSWindow.allowsAutomaticWindowTabbing = false
        Task { @MainActor in
            var fixture: Showcase?
            do {
                let run = try await Showcase(); fixture = run
                let window = NSWindow(contentRect: CGRect(x: 80, y: 60, width: 1280, height: 800),
                    styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
                self.window = window
                window.titleVisibility = .hidden; window.titlebarAppearsTransparent = true
                window.isReleasedWhenClosed = false; window.animationBehavior = .none
                window.contentView = NSHostingView(rootView: EditorView(store: run.store) { MacMetalCanvas(store: run.store) })
                window.setFrame(CGRect(x: 80, y: 60, width: 1280, height: 800), display: true)
                window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                try await run.run()
                NSApp.terminate(nil)
            } catch {
                try? await fixture?.write(JSON(["error": error.localizedDescription, "snapshot": fixture?.store.snapshot.json.raw ?? NSNull()]), "ready.json")
                print("FAIL: native store capture: \(error)"); NSApp.terminate(nil)
            }
        }
    }
}
@main private struct StoreCaptureApp {
    @MainActor static func main() {
        let app = NSApplication.shared, delegate = CaptureDelegate()
        app.setActivationPolicy(.regular); app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}
#else
@MainActor private final class CaptureDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
        options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "StoreCapture", sessionRole: session.role)
        configuration.delegateClass = CaptureSceneDelegate.self
        return configuration
    }
}
@MainActor private final class CaptureSceneDelegate: NSObject, UIWindowSceneDelegate {
    var window: UIWindow?
    let lifecycle = CaptureLifecycle()
    func sceneDidBecomeActive(_ scene: UIScene) { lifecycle.phase = .active }
    func sceneWillResignActive(_ scene: UIScene) { lifecycle.phase = .inactive }
    func sceneDidEnterBackground(_ scene: UIScene) { lifecycle.phase = .background }
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        Task { @MainActor in
            var fixture: Showcase?
            do {
                let run = try await Showcase(); fixture = run
                let host = UIHostingController(rootView: ShowcaseRoot(store: run.store, lifecycle: self.lifecycle))
                let window = UIWindow(windowScene: scene)
                self.window = window; window.rootViewController = host; window.makeKeyAndVisible()
                try await run.run()
            } catch {
                try? await fixture?.write(JSON(["error": error.localizedDescription, "snapshot": fixture?.store.snapshot.json.raw ?? NSNull()]), "ready.json")
                print("FAIL: native store capture: \(error)")
            }
        }
    }
}
@MainActor private final class CaptureLifecycle: ObservableObject {
    @Published var phase: ScenePhase = .inactive
}
private struct ShowcaseRoot: View {
    let store: EditorStore
    @ObservedObject var lifecycle: CaptureLifecycle
    var body: some View {
        GeometryReader { geometry in
            EditorView(store: store) { MetalCanvas(store: store) }
                .frame(width: geometry.size.width, height: geometry.size.height)
        }.ignoresSafeArea().statusBarHidden().modifier(PhoneSystemOverlays())
            .environment(\.scenePhase, lifecycle.phase)
    }
}
@main private struct StoreCaptureApp {
    static func main() { UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(CaptureDelegate.self)) }
}
#endif
