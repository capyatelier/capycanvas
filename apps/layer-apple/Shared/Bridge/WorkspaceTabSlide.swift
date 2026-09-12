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
    private let motion: WorkspaceMotion
    private(set) var grab: Grab?
    init(motion: WorkspaceMotion) { self.motion = motion }
    private func matches(_ tab: JSON, group: UInt64) -> Bool {
        guard let grab, grab.group == group, grab.panels.indices.contains(grab.source) else { return false }
        return !tab.isNull && tab["group"].uint == group && tab["panel"].string == grab.panels[grab.source]["id"].string
    }
    var preview: JSON {
        guard let grab, matches(motion.tab, group: grab.group) else { return JSON() }
        return motion.tab["preview"]
    }
    // The original header only observes visibility, not every preview position.
    func isVisible(in group: UInt64) -> Bool { matches(motion.tabIdentity, group: group) }
    func begin(_ grab: Grab?) { self.grab = grab }
    func clear() { grab = nil }
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
