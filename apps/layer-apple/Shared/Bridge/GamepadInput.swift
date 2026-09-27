import GameController

@MainActor final class GamepadInput {
    static let shared = GamepadInput()
    private var observers: [NSObjectProtocol] = []
    private weak var target: EditorStore?
    private var held: [String: Timer] = [:]
    private var axes: [Double] = [0, 0, 0]

    func start() {
        guard observers.isEmpty else { return }
        observers.append(NotificationCenter.default.addObserver(forName: .GCControllerDidConnect, object: nil, queue: .main) { note in
            MainActor.assumeIsolated { if let controller = note.object as? GCController { GamepadInput.shared.attach(controller) } }
        })
        observers.append(NotificationCenter.default.addObserver(forName: .GCControllerDidDisconnect, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { GamepadInput.shared.release() }
        })
        GCController.controllers().forEach(attach)
    }

    func attach(_ controller: GCController) {
        guard let pad = controller.extendedGamepad else { return }
        let buttons: [(GCControllerButtonInput?, String)] = [
            (pad.buttonA, "a"), (pad.buttonB, "b"), (pad.buttonX, "x"), (pad.buttonY, "y"),
            (pad.leftShoulder, "l1"), (pad.rightShoulder, "r1"), (pad.leftTrigger, "l2"), (pad.rightTrigger, "r2"),
            (pad.buttonOptions, "select"), (pad.buttonMenu, "start"), (pad.leftThumbstickButton, "l3"),
            (pad.rightThumbstickButton, "r3"), (pad.dpad.up, "up"), (pad.dpad.down, "down"), (pad.dpad.left, "left"),
            (pad.dpad.right, "right"), (pad.buttonHome, "home"),
        ]
        for (button, name) in buttons {
            button?.pressedChangedHandler = { _, _, pressed in
                MainActor.assumeIsolated { GamepadInput.shared.press(name, pressed) }
            }
        }
        for stick in [pad.leftThumbstick, pad.rightThumbstick] {
            stick.valueChangedHandler = { [weak pad] _, _, _ in
                MainActor.assumeIsolated { if let pad { GamepadInput.shared.move(pad) } }
            }
        }
    }

    private func store() -> EditorStore? {
        let next = EditorStore.gamepadTarget
        if next !== target { release(); target = next }
        return next
    }

    private func press(_ name: String, _ pressed: Bool) {
        guard let store = store() else { return }
        if pressed {
            guard held[name] == nil else { return }
            send(store, name, pressed: true, repeating: false)
            held[name] = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: false) { _ in
                MainActor.assumeIsolated { GamepadInput.shared.repeatKey(name) }
            }
        } else if let timer = held.removeValue(forKey: name) {
            timer.invalidate()
            send(store, name, pressed: false, repeating: false)
        }
    }

    private func repeatKey(_ name: String) {
        guard held[name] != nil, let store = target else { return }
        send(store, name, pressed: true, repeating: true)
        held[name] = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { _ in
            MainActor.assumeIsolated {
                guard GamepadInput.shared.held[name] != nil, let store = GamepadInput.shared.target else { return }
                GamepadInput.shared.send(store, name, pressed: true, repeating: true)
            }
        }
    }

    private func move(_ pad: GCExtendedGamepad) {
        let next = [Double(pad.leftThumbstick.xAxis.value), Double(-pad.leftThumbstick.yAxis.value), Double(pad.rightThumbstick.yAxis.value)]
            .map { (max(-1, min(1, $0)) * 100).rounded() / 100 }
        guard next != axes, let store = store() else { return }
        axes = next
        store.input(["type": "axes", "pan": [next[0], next[1]], "zoom": next[2]])
    }

    private func release() {
        for (name, timer) in held {
            timer.invalidate()
            if let target { send(target, name, pressed: false, repeating: false) }
        }
        held.removeAll()
        if axes != [0, 0, 0] { target?.input(["type": "axes", "pan": [0, 0], "zoom": 0]) }
        axes = [0, 0, 0]
    }

    private func send(_ store: EditorStore, _ name: String, pressed: Bool, repeating: Bool) {
        store.input(["type": "key", "key": "gamepad_" + name, "pressed": pressed, "repeat": repeating,
            "modifiers": ["command": false, "shift": false, "alt": false]])
    }
}
