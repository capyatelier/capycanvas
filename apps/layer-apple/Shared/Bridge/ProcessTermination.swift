import Foundation

@MainActor final class ProcessTermination {
    enum Outcome: Equatable, Sendable { case complete, incomplete, storagePending }
    private enum State { case idle, flushing, detaching, finishing, done(Outcome) }
    private var state = State.idle
    private let deadline: TimeInterval?
    private let completion: (Outcome) -> Void
    var outcome: Outcome? {
        if case .done(let result) = state { return result }
        return nil
    }
    init(deadline: TimeInterval? = nil, completion: @escaping (Outcome) -> Void = { _ in }) {
        self.deadline = deadline
        self.completion = completion
    }
    func start(flush: (@escaping @MainActor (Bool) -> Void) -> Void,
        detach: @escaping (@escaping @MainActor () -> Void) -> Void,
        finishGPU: @escaping @Sendable (TimeInterval) -> Bool) {
        guard case .idle = state else { return }
        state = .flushing
        flush { saved in
            guard case .flushing = self.state, !self.expireIfNeeded() else { return }
            guard saved else { self.finish(.storagePending); return }
            self.state = .detaching
            detach {
                guard case .detaching = self.state, !self.expireIfNeeded() else { return }
                self.state = .finishing
                let remaining = self.deadline.map { $0 - ProcessInfo.processInfo.systemUptime } ?? 2
                guard remaining > 0 else { self.finish(.incomplete); return }
                let timeout = min(2, remaining)
                DispatchQueue.global(qos: .userInitiated).async {
                    let complete = finishGPU(timeout)
                    DispatchQueue.main.async {
                        guard case .finishing = self.state, !self.expireIfNeeded() else { return }
                        self.finish(complete ? .complete : .incomplete)
                    }
                }
            }
        }
    }
    func wait() -> Outcome {
        guard let deadline else { preconditionFailure("A synchronous termination needs a deadline") }
        while outcome == nil && !expireIfNeeded() {
            let remaining = deadline - ProcessInfo.processInfo.systemUptime
            _ = RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: min(0.01, max(0, remaining))))
        }
        return outcome!
    }
    private func expireIfNeeded() -> Bool {
        guard let deadline, ProcessInfo.processInfo.systemUptime >= deadline else { return false }
        if case .finishing = state { finish(.incomplete) }
        else { finish(.storagePending) }
        return true
    }
    private func finish(_ result: Outcome) {
        state = .done(result)
        completion(result)
    }
}
