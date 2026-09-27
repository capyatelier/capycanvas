import Foundation
import GameController
import QuartzCore

@main struct GamepadInputChecks {
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-gamepad-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, scene: UUID().uuidString, persistence: EditorPersistence(root: root))
            try await wait("Workspace startup") { store.workspaces?.ready == true || store.failure != nil }
            try require(store.failure == nil, store.failure ?? "")
            let surface = attachSurface(store, CGSize(width: 256, height: 192))
            try await wait("Metal startup", failure: { store.failure }, step: {
                try await prepare(store.native!, "Metal startup")
                await frame(store.native!)
            }) { store.snapshot["shaders_ready"].bool }
            store.acceptsGamepad = { true }
            let controller = GCController.withExtendedGamepad()
            GamepadInput.shared.attach(controller)
            let pad = try unwrap(controller.extendedGamepad, "Snapshot gamepad")
            func press(_ button: GCControllerButtonInput) { button.setValue(1); button.setValue(0) }

            try await store.apply(["type": "invoke", "command": "settings"])
            try await store.apply(["type": "preferences", "action": ["type": "begin_shortcut", "id": "command.ZenMode"]])
            press(pad.buttonA)
            try await wait("Gamepad button recording") { store.snapshot["preferences"]["capture"]["chord"]["key"].string == "gamepad_a" }
            try await store.apply(["type": "preferences", "action": ["type": "confirm_shortcut", "replace": false]])
            try await store.apply(["type": "close_settings"])
            try require(!store.state["workspace"]["zen_mode"].bool, "Zen starts off")
            press(pad.buttonA)
            try await wait("Gamepad shortcut") { store.state["workspace"]["zen_mode"].bool }
            press(pad.buttonA)
            try await wait("Gamepad shortcut again") { !store.state["workspace"]["zen_mode"].bool }

            let camera = store.state["camera"]["translation"].stableKey
            pad.leftThumbstick.setValueForXAxis(1, yAxis: 0)
            try await wait("Left stick pan", step: { await frame(store.native!) }) { store.state["camera"]["translation"].stableKey != camera }
            pad.leftThumbstick.setValueForXAxis(0, yAxis: 0)
            let zoom = store.state["camera"]["zoom"].number
            pad.rightThumbstick.setValueForXAxis(0, yAxis: 1)
            try await wait("Right stick zoom", step: { await frame(store.native!) }) { store.state["camera"]["zoom"].number > zoom }
            pad.rightThumbstick.setValueForXAxis(0, yAxis: 0)
            store.acceptsGamepad = nil
            withExtendedLifetime(surface) {}
            print("PASS: platform \(platform), gamepad button recording, shortcut, repeat-free press, stick pan and zoom")
        }
    }
    static func unwrap<T>(_ value: T?, _ label: String) throws -> T {
        guard let value else { throw HostFailure(message: "Missing \(label)") }
        return value
    }
}
