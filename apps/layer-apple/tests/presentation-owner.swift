// Inject pending presentation tickets through an actually attached Metal layer,
// then exercise the real shared owner's resize/resume/detach paths. No window.
import Foundation
import QuartzCore
import Darwin

private func require(_ value: Bool, _ message: String) {
    if !value { FileHandle.standardError.write(Data("FAIL: \(message)\n".utf8)); exit(1) }
}

@main struct PresentationOwnerChecks {
    static func barrier(_ owner: NativeOwner) {
        let done = DispatchSemaphore(value: 0)
        owner.submit(2, JSON(["type": "catalog"])) { value in
            require(value != nil, "Native owner query failed"); done.signal()
        }
        require(done.wait(timeout: .now() + 20) == .success, "Native owner timed out")
    }
    static func fill(_ gate: FramePresentationGate, count: Int) -> [UInt64] {
        let tickets = (0..<count).map { _ in gate.acquired() }
        require(!gate.hasCapacity, "Pending drawables did not close admission")
        return tickets
    }
    static func main() throws {
        for platform: UInt32 in [0, 1] {
            let owner = try NativeOwner(platform: platform,
                persistence: EditorPersistence(root: nil), receive: { _, error in
                    require(error == nil, error ?? "")
                })
            let layer = ObservedMetalLayer()
            layer.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            owner.attach(layer, width: 128, height: 128, scale: 1); barrier(owner)
            guard let gate = layer.presentationGate else { fatalError("Attached layer lacks its owner's gate") }
            let count = layer.maximumDrawableCount
            let cancelled = DispatchSemaphore(value: 0)
            let old = fill(gate, count: count)
            require(!owner.canAdmitPresentation, "The owner must observe its attached layer")
            owner.whenPresentationAvailable { cancelled.signal() }
            owner.resize(width: 160, height: 128, scale: 1); barrier(owner)
            require(owner.canAdmitPresentation, "Resize must invalidate discarded drawable tickets")
            let resized = fill(gate, count: count)
            for ticket in old { gate.retired(ticket) }
            require(!owner.canAdmitPresentation, "Old-size callbacks retired new-size tickets")
            owner.whenPresentationAvailable { cancelled.signal() }
            owner.invalidatePresentations()
            require(owner.canAdmitPresentation, "Resume must invalidate discarded drawable tickets")
            let resumed = fill(gate, count: count)
            for ticket in resized { gate.retired(ticket) }
            require(!owner.canAdmitPresentation, "Pre-suspension callbacks retired resumed tickets")
            owner.whenPresentationAvailable { cancelled.signal() }
            owner.detach(); barrier(owner)
            require(layer.presentationGate == nil && owner.canAdmitPresentation, "Detach must release admission and disconnect the layer")
            let replacement = ObservedMetalLayer()
            replacement.bounds = layer.bounds
            owner.attach(replacement, width: 128, height: 128, scale: 1); barrier(owner)
            require(replacement.presentationGate === gate, "The owner must retain one gate across surface replacement")
            let latest = fill(gate, count: replacement.maximumDrawableCount)
            for ticket in resumed { gate.retired(ticket) }
            require(!owner.canAdmitPresentation, "Detached-layer callbacks retired replacement tickets")
            require(cancelled.wait(timeout: .now()) == .timedOut,
                "Resize, resume and detach must retire old capacity waiters")
            let notified = DispatchSemaphore(value: 0)
            owner.whenPresentationAvailable { notified.signal() }
            for ticket in latest { gate.retired(ticket) }
            require(owner.canAdmitPresentation, "Completed replacement drawables must reopen admission")
            require(notified.wait(timeout: .now()) == .success && notified.wait(timeout: .now()) == .timedOut,
                "The actual owner's replacement layer must notify capacity exactly once")
            owner.detach(); barrier(owner)
            print("PASS platform \(platform): real Metal attach, pending admission, resize, resume, detach and replacement")
        }
    }
}
