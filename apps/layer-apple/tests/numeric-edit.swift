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
    }
}
