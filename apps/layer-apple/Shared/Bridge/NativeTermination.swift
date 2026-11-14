import Foundation
import Darwin

extension ProcessTermination {
    nonisolated static func finishGPU(_ timeout: TimeInterval) -> Bool {
        capy_apple_finish_process_gpu(UInt64(max(0, min(2, timeout)) * 1000))
    }
    static func exitIfIncomplete(_ result: Outcome) {
        guard capy_apple_termination_requires_exit(result != .storagePending, result == .complete) else { return }
        capy_apple_termination_incomplete()
        Darwin._exit(0)
    }
}
