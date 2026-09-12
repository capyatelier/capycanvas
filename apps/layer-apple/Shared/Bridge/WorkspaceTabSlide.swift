import SwiftUI
import Observation

/// Native measurements are captured once; Rust owns switch points, clamping,
/// neighbor offsets and the committed drop. Publication stays inside tab views.
@Observable @MainActor final class WorkspaceTabSlide {
    struct Grab {
        let id = UUID()
        let group: UInt64, source: Int
        let frames: [WorkspaceTabFrame]
        let clip: CGRect
        let panels: [JSON]
        let active: String
        var action: [String: Any] {
            ["type": "begin_tab_drag", "clip": JSON(clip).raw,
             "tabs": frames.enumerated().map { index, frame in
                 ["group": group, "index": index, "bounds": JSON(frame.bounds).raw] as [String: Any]
             }]
        }
    }
    private(set) var preview = JSON()
    private(set) var grab: Grab?
    func begin(_ grab: Grab?) { self.grab = grab; preview = JSON() }
    func receive(_ value: JSON) {
        if !SnapshotProjection.equal(preview.raw, value.raw) { preview = value }
    }
    func clear() { grab = nil; preview = JSON() }
    func offset(group: UInt64, index: Int) -> Double {
        guard !preview.isNull, let grab, grab.group == group, grab.frames.indices.contains(index) else { return 0 }
        if grab.source == index { return preview["bounds"]["x"].number - grab.frames[index].bounds.minX }
        return preview["offsets"].array.first { $0["index"].uint == UInt64(index) }?["x"].number ?? 0
    }
}

struct WorkspaceTabFrame: Equatable {
    let bounds: CGRect, clip: CGRect
}
struct WorkspaceTabFrames: PreferenceKey {
    static var defaultValue: [String: WorkspaceTabFrame] { [:] }
    static func reduce(value: inout [String: WorkspaceTabFrame], nextValue: () -> [String: WorkspaceTabFrame]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}
