// swiftc Shared/Editor/NumericEditState.swift tests/numeric-edit.swift -o /tmp/capy-numeric-edit
import Foundation

@main struct NumericEditChecks {
    static func main() {
        var field = NumericEditState()
        field.receive(10)
        field.text = "2 * (3 +"
        field.dirty = true
        field.receive(20)
        precondition(field.text == "2 * (3 +" && field.dirty, "Snapshots must preserve an unfinished expression")

        let first = field.submit(21)
        let second = field.submit(22)
        field.receive(21)
        field.complete(first, error: nil)
        precondition(field.value == 22, "An older acknowledgment must not undo a rapid second step")
        field.receive(22)
        field.complete(second, error: nil)
        precondition(field.value == 22)

        let rejected = field.submit(0)
        field.complete(rejected, error: "Scale cannot be zero")
        precondition(field.value == 22 && field.error == "Scale cannot be zero")
        let corrected = field.submit(23.7)
        field.receive(Double(Float(23.7)))
        field.complete(corrected, error: nil)
        precondition(field.value == Double(Float(23.7)) && field.error == nil)
        field.receive(42)
        precondition(field.value == 42, "External edits must resume after acknowledgment")

        let bad = field.submit(0)
        let good = field.submit(43)
        field.complete(bad, error: "Old failure")
        precondition(field.value == 43 && field.error == nil)
        field.receive(43)
        field.complete(good, error: nil)
        precondition(field.value == 43 && field.error == nil)
        print("Numeric edit checks passed: draft preservation, rapid edits, rollback and f32 acknowledgment")
        replacementOwnership(skipLateUnmount: CommandLine.arguments.contains("--late-update"))
        selectedAdmissions()
        print("Numeric admission checks passed: replacement ownership, late lifecycle events, selected discard and complete validation")
    }

    private static func check(_ accepted: Bool, _ message: String) {
        if !accepted {
            FileHandle.standardError.write(Data("FAIL: \(message)\n".utf8))
            exit(1)
        }
    }

    private static func replacementOwnership(skipLateUnmount: Bool) {
        var admissions = NumericAdmissions()
        let old = UUID(), current = UUID()
        var oldCalls: [Bool] = [], currentCalls: [Bool] = [], lateCalls: [Bool] = [], updatedCalls: [Bool] = []
        admissions.receive("width", .mounted(old, { discard in oldCalls.append(discard); return true }))
        admissions.receive("width", .mounted(current, { discard in currentCalls.append(discard); return discard }))
        if !skipLateUnmount { admissions.receive("width", .unmounted(old)) }
        check(!admissions.finish(), skipLateUnmount ? "The replacement field must retain its invalid-draft veto"
            : "A late old unmount must retain the replacement field's invalid-draft veto")
        check(oldCalls.isEmpty && currentCalls == [false], "Only the replacement field may validate the shared key")
        admissions.receive("width", .updated(old, { discard in lateCalls.append(discard); return true }))
        check(!admissions.finish(only: "width"), "A late old update must not reclaim the replacement's admission")
        check(lateCalls.isEmpty && currentCalls == [false, false], "Retired callbacks must never receive validation")
        check(admissions.finish(true, only: "width") && currentCalls == [false, false, true]
            && oldCalls.isEmpty && lateCalls.isEmpty, "Discard must reach only the current replacement field")
        admissions.receive("width", .updated(current, { discard in updatedCalls.append(discard); return true }))
        check(admissions.finish(), "The current owner may replace its admission after correcting the draft")
        check(updatedCalls == [false] && currentCalls.count == 3, "A current update must retire its previous callback")
        admissions.receive("width", .unmounted(old))
        check(admissions.finish() && updatedCalls == [false, false], "Repeated stale unmount must preserve the current updated callback")
        admissions.receive("width", .unmounted(current))
        check(admissions.finish() && updatedCalls.count == 2, "Current unmount must remove its field without invoking it")
        admissions.receive("width", .updated(current, { discard in lateCalls.append(discard); return false }))
        check(admissions.finish() && lateCalls.isEmpty, "An update after current unmount must not resurrect the field")
    }

    private static func selectedAdmissions() {
        var admissions = NumericAdmissions()
        var width: [Bool] = [], height: [Bool] = [], relief: [Bool] = []
        admissions.receive("width", .mounted(UUID(), { discard in width.append(discard); return discard }))
        admissions.receive("height", .mounted(UUID(), { discard in height.append(discard); return true }))
        admissions.receive("relief", .mounted(UUID(), { discard in relief.append(discard); return true }))
        check(admissions.finish(only: "missing") && width.isEmpty && height.isEmpty && relief.isEmpty,
            "An absent selected key must not validate unrelated fields")
        check(!admissions.finish(only: "width") && width == [false] && height.isEmpty && relief.isEmpty,
            "Per-key admission must retain the selected field's invalid-draft veto")
        check(admissions.finish(true, only: "relief") && relief == [true] && width == [false] && height.isEmpty,
            "Discard must be delivered exactly once to the selected field only")
        check(!admissions.finish(), "An invalid selected field must veto the combined operation")
        check(width == [false, false] && height == [false] && relief == [true, false],
            "Every selected field must be attempted exactly once even when another rejects")
        check(admissions.finish(true), "Discard must reach every current field without validation veto")
        check(width == [false, false, true] && height == [false, true] && relief == [true, false, true],
            "Combined discard must be delivered once to every current field")
        check(admissions.finish(only: "height") && height == [false, true, false]
            && width == [false, false, true] && relief == [true, false, true],
            "Selected validation must leave other fields' lifecycle and callbacks untouched")
        var allInvalid = NumericAdmissions()
        var attempts: [String: Int] = [:]
        for key in ["width", "height", "relief"] {
            allInvalid.receive(key, .mounted(UUID(), { discard in attempts[key, default: 0] += 1; return discard }))
        }
        check(!allInvalid.finish() && attempts == ["width": 1, "height": 1, "relief": 1],
            "Validation must visit every rejecting field regardless of dictionary iteration order")
    }
}
