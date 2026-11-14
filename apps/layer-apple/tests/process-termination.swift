import Foundation

@MainActor final class TerminationEvents {
    var values: [String] = []
}

@main struct ProcessTerminationChecks {
    @MainActor static func main() {
        mainActorFlushCompletesDuringSynchronousTermination()
        finishWaitsForFlushAndDetach()
        undurableDeadlineReturnsNormally()
        detachDeadlineReturnsNormally()
        durableDeadlineSelectsIncomplete()
        oneDeadlineIncludesStorageAndGPU()
        failedFlushCannotDetachOrFinishGPU()
        print("Process termination checks passed: ordering, MainActor progress, overall deadline and completion")
    }
    @MainActor static func mainActorFlushCompletesDuringSynchronousTermination() {
        let attempt = ProcessTermination(deadline: ProcessInfo.processInfo.systemUptime + 0.2)
        attempt.start(flush: { callback in
            DispatchQueue.main.async { callback(true) }
        }, detach: { callback in
            DispatchQueue.main.async { callback() }
        }, finishGPU: { timeout in
            precondition(timeout > 0 && timeout <= 2)
            return true
        })
        precondition(attempt.wait() == .complete, "MainActor flush and detach must run during the termination wait")
    }
    @MainActor static func finishWaitsForFlushAndDetach() {
        let events = TerminationEvents()
        var flushed: (@MainActor (Bool) -> Void)?
        var detached: (@MainActor () -> Void)?
        let attempt = ProcessTermination(deadline: ProcessInfo.processInfo.systemUptime + 0.2)
        attempt.start(flush: { callback in events.values.append("flush"); flushed = callback },
            detach: { callback in events.values.append("detach"); detached = callback },
            finishGPU: { _ in true })
        precondition(events.values == ["flush"] && attempt.outcome == nil)
        flushed!(true)
        precondition(events.values == ["flush", "detach"] && attempt.outcome == nil)
        detached!()
        precondition(attempt.wait() == .complete)
    }
    @MainActor static func undurableDeadlineReturnsNormally() {
        var flushed: (@MainActor (Bool) -> Void)?
        var detaches = 0
        let attempt = ProcessTermination(deadline: ProcessInfo.processInfo.systemUptime + 0.03)
        attempt.start(flush: { flushed = $0 }, detach: { callback in detaches += 1; callback() },
            finishGPU: { _ in preconditionFailure("Undurable storage cannot start GPU termination") })
        precondition(attempt.wait() == .storagePending)
        flushed!(true)
        precondition(detaches == 0 && attempt.outcome == .storagePending)
    }
    @MainActor static func detachDeadlineReturnsNormally() {
        let attempt = ProcessTermination(deadline: ProcessInfo.processInfo.systemUptime + 0.03)
        attempt.start(flush: { $0(true) }, detach: { _ in },
            finishGPU: { _ in preconditionFailure("Pending detach cannot start GPU termination") })
        precondition(attempt.wait() == .storagePending)
    }
    @MainActor static func oneDeadlineIncludesStorageAndGPU() {
        let release = DispatchSemaphore(value: 0), exited = DispatchSemaphore(value: 0)
        let started = ProcessInfo.processInfo.systemUptime
        let attempt = ProcessTermination(deadline: started + 0.15)
        attempt.start(flush: { callback in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.04) { callback(true) }
        }, detach: { callback in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.04) { callback() }
        }, finishGPU: { timeout in
            precondition(timeout > 0 && timeout < 0.09, "Storage must consume the same overall deadline")
            release.wait()
            exited.signal()
            return true
        })
        precondition(attempt.wait() == .incomplete)
        precondition(ProcessInfo.processInfo.systemUptime - started < 0.2)
        release.signal()
        precondition(exited.wait(timeout: .now() + 1) == .success)
    }
    @MainActor static func failedFlushCannotDetachOrFinishGPU() {
        let attempt = ProcessTermination()
        attempt.start(flush: { $0(false) }, detach: { _ in preconditionFailure("Failed flush cannot detach") },
            finishGPU: { _ in preconditionFailure("Failed flush cannot stop GPU services") })
        precondition(attempt.outcome == .storagePending)
    }
    @MainActor static func durableDeadlineSelectsIncomplete() {
        let release = DispatchSemaphore(value: 0)
        let exited = DispatchSemaphore(value: 0)
        let attempt = ProcessTermination(deadline: ProcessInfo.processInfo.systemUptime + 0.03)
        attempt.start(flush: { $0(true) }, detach: { $0() }, finishGPU: { _ in
            release.wait()
            exited.signal()
            return true
        })
        let started = ProcessInfo.processInfo.systemUptime
        precondition(attempt.wait() == .incomplete)
        precondition(ProcessInfo.processInfo.systemUptime - started < 0.08)
        release.signal()
        precondition(exited.wait(timeout: .now() + 1) == .success)
        precondition(attempt.outcome == .incomplete)
    }
}
