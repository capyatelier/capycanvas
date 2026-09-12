import Foundation
import Observation

/// A stable observation identity, separate from the immutable snapshot value.
/// UI mutation stays on the main actor; no mutable payload crosses queues.
@MainActor final class SnapshotSignal: Observable {
    private let registrar = ObservationRegistrar()
    private var revision: UInt64 = 0
    private var value: UInt64 { registrar.access(self, keyPath: \.value); return revision }
    func read() { _ = value }
    func publish() { registrar.withMutation(of: self, keyPath: \.value) { revision &+= 1 } }
}

/// A live, main-actor field reader for SwiftUI. Each subscript returns immutable
/// JSON and tracks that field; `json` snapshots the whole object and tracks all
/// its fields. A child retaining this reader gets fresh data when invalidated,
/// while a retained JSON value always keeps its original data. Stage related
/// readers before publishing so observers see one coherent editor state.
@MainActor final class SnapshotProjection {
    private(set) var unobserved = JSON()
    private var fields: [String: SnapshotSignal] = [:]
    private let all = SnapshotSignal(), empty = SnapshotSignal()
    private var wholeWasRead = false
    var json: JSON { wholeWasRead = true; all.read(); return unobserved }
    var isNull: Bool { empty.read(); return unobserved.isNull }
    subscript(_ key: String) -> JSON {
        if fields[key] == nil { fields[key] = SnapshotSignal() }
        fields[key]!.read()
        return unobserved[key]
    }
    func stage(_ next: JSON) -> [SnapshotSignal] { stage(next, candidates: nil) }
    func stagePatch(_ updates: [String: JSON]) -> [SnapshotSignal] {
        var next = unobserved.object
        for (key, value) in updates { next[key] = value.raw }
        return stage(JSON(next), candidates: Set(updates.keys))
    }
    private func stage(_ next: JSON, candidates: Set<String>?) -> [SnapshotSignal] {
        precondition(next.isNull || next.raw is NSDictionary, "A projection root must be an object or null")
        let before = unobserved.object, after = next.object
        var changed: [SnapshotSignal] = []
        // Unread fields still enter the canonical snapshot, but have no view to
        // notify. Avoid walking their nested values or allocating signal nodes.
        for (key, signal) in fields where candidates == nil || candidates!.contains(key) {
            guard (before[key] == nil) != (after[key] == nil)
                || !Self.equal(before[key] ?? NSNull(), after[key] ?? NSNull()) else { continue }
            changed.append(signal)
        }
        if unobserved.isNull != next.isNull { changed.append(empty) }
        // Whole-object readers also observe fields that no individual view
        // requested. A known field change already proves the object changed.
        if wholeWasRead && (!changed.isEmpty || !Self.equal(unobserved.raw, next.raw)) { changed.append(all) }
        unobserved = next
        return changed
    }
    static func indexed(_ values: JSON) -> JSON {
        var result: [String: Any] = [:]
        for value in values.array where result[value["id"].string] == nil {
            result[value["id"].string] = value.raw
        }
        return JSON(result)
    }

    /// JSON value equality, including Boolean/number distinction, precise
    /// integers and signed zero. Foundation container equality alone conflates
    /// true with 1, which would suppress a real transport change.
    static func equal(_ left: Any, _ right: Any) -> Bool {
        if left is NSNull { return right is NSNull }
        if let left = left as? NSNumber, let right = right as? NSNumber {
            guard (CFGetTypeID(left) == CFBooleanGetTypeID()) == (CFGetTypeID(right) == CFBooleanGetTypeID()) else { return false }
            if left.doubleValue == 0 && right.doubleValue == 0 && left.doubleValue.sign != right.doubleValue.sign { return false }
            return left == right
        }
        if let left = left as? NSString, let right = right as? NSString { return left == right }
        if let left = left as? NSDictionary, let right = right as? NSDictionary {
            if left === right { return true }
            guard left.count == right.count else { return false }
            for key in left.allKeys {
                guard let key = key as? String, let value = right[key], let original = left[key], equal(original, value) else { return false }
            }
            return true
        }
        if let left = left as? NSArray, let right = right as? NSArray {
            if left === right { return true }
            return left.count == right.count && (0..<left.count).allSatisfy { equal(left[$0], right[$0]) }
        }
        return false
    }
}
