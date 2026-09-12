// Exercise the real serial owner without a window, GPU or artist persistence.
import Foundation
import Darwin

private func require(_ condition: Bool, _ message: String) {
    guard condition else {
        FileHandle.standardError.write(Data("FAIL: \(message)\n".utf8))
        exit(1)
    }
}

private final class Retirements: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    private let complete = DispatchSemaphore(value: 0)
    private let expected: Int
    init(expected: Int) { self.expected = expected }
    var retired: Int { lock.lock(); defer { lock.unlock() }; return count }
    func retire() {
        lock.lock(); count += 1; let done = count == expected; lock.unlock()
        if done { complete.signal() }
    }
    func wait() {
        require(complete.wait(timeout: .now() + 15) == .success,
            "The last owner task must drain its temporary resources before idle")
    }
}

private final class TemporaryResource: NSObject {
    let retirements: Retirements
    init(_ retirements: Retirements) { self.retirements = retirements }
    deinit { retirements.retire() }
}

@main struct OwnerAutoreleaseChecks {
    static func main() throws {
        for platform: UInt32 in [0, 1] {
            let retired = Retirements(expected: 64)
            let owner = try NativeOwner(platform: platform, scene: UUID().uuidString,
                persistence: EditorPersistence(root: nil), receive: { _, error in
                    require(error == nil, error ?? "")
                })
            for index in 0..<64 {
                owner.submit(2, JSON(["type": "catalog"])) { reply in
                    require(reply != nil, "The real owner request must succeed")
                    require(retired.retired == index,
                        "Native temporary resources survived their owner task")
                    // Simulate a native autoreleased return value without
                    // retaining it in the caller. Its lifetime belongs to the
                    // actual owner's pool, not a test-local autorelease block.
                    _ = Unmanaged.passRetained(TemporaryResource(retired)).autorelease()
                }
            }
            retired.wait()
            withExtendedLifetime(owner) {}
            print("PASS platform \(platform): 64 real owner tasks drain temporaries, including the final idle boundary")
        }
    }
}
