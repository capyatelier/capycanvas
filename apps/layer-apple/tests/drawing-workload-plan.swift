import Foundation

@main struct WorkloadPlanChecks {
    static func main() throws {
        for name in ["ink", "ink-predicted", "wet-watercolor", "layered-4k", "wet-watercolor-4k"] {
            let plan = try DrawingWorkloadPlan(name: name, seconds: 600)
            var contacts: [UInt64: [DrawingWorkloadPlan.Sample]] = [:]
            for index in 0..<(DrawingWorkloadPlan.cycleSamples * 3) {
                guard let sample = plan.sample(index) else { continue }
                precondition(sample.x > 0 && sample.x < Double(plan.extent))
                precondition(sample.y > 0 && sample.y < Double(plan.extent))
                precondition(sample.pressure >= 0.25 && sample.pressure <= 1)
                contacts[sample.contact, default: []].append(sample)
            }
            precondition(contacts.count == 3)
            for contact in contacts.values {
                precondition(contact.count == 360)
                precondition(contact.first?.phase == 1 && contact.last?.phase == 3)
                precondition(contact.dropFirst().dropLast().allSatisfy { $0.phase == 2 })
            }
            precondition(plan.sample(360) == nil && plan.sample(383) == nil)
            precondition(plan.sample(384)?.contact == 2 && plan.sample(384)?.phase == 1)
        }
        for seconds in [Double.nan, Double.infinity, 0, 1801] {
            do { _ = try DrawingWorkloadPlan(name: "ink", seconds: seconds); preconditionFailure("Invalid duration accepted") }
            catch {}
        }
        do { _ = try DrawingWorkloadPlan(name: "unknown", seconds: 600); preconditionFailure("Unknown workload accepted") }
        catch {}
        print("PASS: workload contact termination, lift gaps, bounds, pressure, and configuration validation")
    }
}
