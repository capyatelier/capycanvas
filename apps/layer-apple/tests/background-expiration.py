"""Check the exact production background helper against a synchronous UIKit-shaped boundary.

No simulator or app launch: this proves callback ordering and task ownership,
not physical OS expiration or recovery durability. Run from the repository root.
"""
import os, subprocess, tempfile
from pathlib import Path

source = Path('apps/layer-apple/iOS/App/CapyCanvasApp.swift').read_text()
marker = '@MainActor private enum PersistenceBackground {'
assert source.count(marker) == 1
helper = source[source.index(marker):]
stub = r'''
import Foundation

struct UIBackgroundTaskIdentifier: Hashable {
    let rawValue: Int
    static let invalid = Self(rawValue: 0)
}

@MainActor final class UIApplication {
    static let shared = UIApplication()
    var admitted = true
    var handlers: [UIBackgroundTaskIdentifier: @MainActor @Sendable () -> Void] = [:]
    var ended: [UIBackgroundTaskIdentifier] = []
    var next = 0
    func beginBackgroundTask(withName: String?, expirationHandler: (@MainActor @Sendable () -> Void)?) -> UIBackgroundTaskIdentifier {
        guard admitted else { return .invalid }
        next += 1
        let id = UIBackgroundTaskIdentifier(rawValue: next)
        handlers[id] = expirationHandler
        return id
    }
    func endBackgroundTask(_ identifier: UIBackgroundTaskIdentifier) { ended.append(identifier) }
    var latest: UIBackgroundTaskIdentifier { .init(rawValue: next) }
}

@MainActor final class EditorStore {
    var immediately: Bool?
    var completion: (@MainActor (Bool) -> Void)?
    func flushPersistence(_ completion: @escaping @MainActor (Bool) -> Void) {
        if let immediately { completion(immediately) }
        else { self.completion = completion }
    }
}

@main enum Checks {
    @MainActor static func require(_ value: Bool, _ message: String) {
        if !value { print("FAIL: \(message)"); exit(1) }
    }
    @MainActor static func main() {
        let app = UIApplication.shared
        let pending = EditorStore()
        PersistenceBackground.flush(pending)
        let first = app.latest
        require(app.ended.isEmpty, "Pending persistence must keep its background allowance")
        app.handlers[first]!()
        require(app.ended == [first], "Expiration must end its allowance before the synchronous handler returns")
        pending.completion!(true)
        app.handlers[first]!()
        require(app.ended == [first], "Late completion and duplicate expiration must not end the allowance twice")
        print("PASS: expiration finishes synchronously; late completion is idempotent")

        let completed = EditorStore()
        PersistenceBackground.flush(completed)
        let second = app.latest
        completed.completion!(false)
        require(app.ended == [first, second], "Failed persistence must release its allowance too")
        app.handlers[second]!()
        require(app.ended == [first, second], "Expiration after completion must be harmless")
        print("PASS: completion before expiration and failed persistence release exactly once")

        let immediate = EditorStore(); immediate.immediately = true
        PersistenceBackground.flush(immediate)
        let third = app.latest
        require(app.ended == [first, second, third], "An immediate persistence completion must release its assigned allowance")
        print("PASS: immediate persistence completion releases its allowance")

        app.admitted = false
        let denied = EditorStore(); denied.immediately = false
        PersistenceBackground.flush(denied)
        require(app.ended == [first, second, third], "Denied background execution must not end an invalid identifier")
        app.admitted = true
        print("PASS: invalid background-task identifier is not ended")

        let left = EditorStore(), right = EditorStore()
        PersistenceBackground.flush(left); let leftID = app.latest
        PersistenceBackground.flush(right); let rightID = app.latest
        app.handlers[rightID]!()
        left.completion!(true)
        right.completion!(true)
        require(app.ended == [first, second, third, rightID, leftID], "Independent scenes must release only their own allowance once")
        print("PASS: overlapping scenes keep independent completion and expiration lifetimes")
    }
}
'''
with tempfile.TemporaryDirectory(prefix='capy-background-expiration-') as temporary:
    directory = Path(temporary)
    check = directory / 'check.swift'; check.write_text(stub + '\n' + helper)
    executable = directory / 'check'
    env = os.environ | {'DEVELOPER_DIR': '/Applications/Xcode.app/Contents/Developer'}
    subprocess.run(['xcrun','swiftc','-parse-as-library','-swift-version','6','-module-cache-path',str(directory/'modules'),str(check),'-o',str(executable)],env=env,check=True)
    result = subprocess.run([str(executable)],env=env)
    raise SystemExit(result.returncode)
