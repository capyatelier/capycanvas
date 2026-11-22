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
                let point = index % 384, stroke = index / 384
                let fraction = Double(point) / 359, shift = Double(stroke % 13) * 0.37
                precondition(sample.x == Double(plan.extent) * (0.5 + 0.32 * sin(fraction * .pi * 2 + shift)))
                precondition(sample.y == Double(plan.extent) * (0.5 + 0.27 * sin(fraction * .pi * 3 + shift * 0.7)))
                precondition(sample.pressure == 0.25 + 0.75 * sin(fraction * .pi))
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
        let environment = ["CAPY_WORKLOAD": "photo", "CAPY_WORKLOAD_DOCUMENT": "/owned/reference.capy",
            "CAPY_WORKLOAD_BRUSH": "21", "CAPY_WORKLOAD_DIAMETER": "512", "CAPY_WORKLOAD_THEME": "light"]
        let photo = try DrawingWorkloadPlan.configured(environment: environment)!
        precondition(photo.seconds == 20 && photo.documentExtent == [9504, 6336] && photo.theme == "light")
        precondition(photo.brush == 21 && photo.diameter == 512 && !photo.predicts)
        precondition(photo.document == "/owned/reference.capy")
        precondition(!String(describing: photo.metadata).contains("/owned/"))
        var photoContacts: [UInt64: [DrawingWorkloadPlan.Sample]] = [:]
        for index in 0..<(20 * DrawingWorkloadPlan.samplesPerSecond) {
            guard let sample = photo.sample(index) else { continue }
            precondition(sample.x > 0 && sample.x < 9504 && sample.y > 0 && sample.y < 6336)
            precondition(sample.pressure == 1)
            photoContacts[sample.contact, default: []].append(sample)
        }
        precondition(Set(photoContacts.keys) == Set([UInt64(1), 2, 3]))
        for contact in photoContacts.values {
            precondition(contact.count == 1440)
            precondition(contact.first?.phase == 1 && contact.last?.phase == 3)
            precondition(contact.dropFirst().dropLast().allSatisfy { $0.phase == 2 })
        }
        for start in [1440, 3120] {
            precondition((start..<(start + 240)).allSatisfy { photo.sample($0) == nil })
        }
        precondition(photo.sample(-1) == nil && photo.sample(4800) == nil)
        for (key, values) in ["CAPY_WORKLOAD_SECONDS": ["19", "21", "nan"], "CAPY_WORKLOAD_THEME": ["", "system", "Light"],
            "CAPY_WORKLOAD_DOCUMENT": ["", "relative.capy", "/owned/reference.jpg"],
            "CAPY_WORKLOAD_BRUSH": ["", "0", "-1", "4294967296"],
            "CAPY_WORKLOAD_DIAMETER": ["", "nan", "inf", "0.49", "2049"]] {
            for value in values {
                var invalid = environment; invalid[key] = value
                do { _ = try DrawingWorkloadPlan.configured(environment: invalid); preconditionFailure("Invalid photo configuration accepted") }
                catch {}
            }
        }
        for key in ["CAPY_WORKLOAD_DOCUMENT", "CAPY_WORKLOAD_BRUSH", "CAPY_WORKLOAD_DIAMETER"] {
            var invalid = environment; invalid.removeValue(forKey: key)
            do { _ = try DrawingWorkloadPlan.configured(environment: invalid); preconditionFailure("Missing photo configuration accepted") }
            catch {}
        }
        for seconds in [Double.nan, Double.infinity, 0, 1801] {
            do { _ = try DrawingWorkloadPlan(name: "ink", seconds: seconds); preconditionFailure("Invalid duration accepted") }
            catch {}
        }
        do { _ = try DrawingWorkloadPlan(name: "unknown", seconds: 600); preconditionFailure("Unknown workload accepted") }
        catch {}
        let chainEnvironment = ["CAPY_WORKLOAD": "chain", "CAPY_WORKLOAD_CHAIN": "Documents/capy-bench/chain.json",
            "CAPY_WORKLOAD_OUTPUT": "/owned/pass-2", "CAPY_WORKLOAD_PASS": "2", "CAPY_WORKLOAD_COLD": "0",
            "CAPY_WORKLOAD_ROOT": "bench-main", "CAPY_WORKLOAD_HEAVY": "Documents/capy-bench/heavy.capy",
            "CAPY_WORKLOAD_STEPS": "launch,open.heavy", "CAPY_WORKLOAD_DEVICE": "macmini"]
        let warm = try DrawingWorkloadPlan.configured(environment: chainEnvironment)!
        let chain = warm.chain!
        precondition(warm.seconds == 1200 && warm.document == nil && chain.pass == 2 && !chain.cold && chain.root == "bench-main")
        precondition(chain.file.path == NSHomeDirectory() + "/Documents/capy-bench/chain.json" && chain.output.path == "/owned/pass-2")
        precondition(chain.document("heavy")?.path == NSHomeDirectory() + "/Documents/capy-bench/heavy.capy" && chain.document("photo") == nil)
        precondition(chain.steps == ["launch", "open.heavy"] && chain.label("DEVICE") == "macmini" && chain.label("COMMIT") == nil)
        precondition(warm.persistenceRoot.path.hasSuffix("/CapyPerformanceSessions/bench-main"))
        precondition(!String(describing: warm.metadata).contains("capy-bench") && !String(describing: warm.metadata).contains("owned"))
        var coldEnvironment = chainEnvironment; coldEnvironment["CAPY_WORKLOAD_COLD"] = "1"
        let cold = try DrawingWorkloadPlan.configured(environment: coldEnvironment)!.chain!
        precondition(cold.cold && cold.root != "bench-main" && UUID(uuidString: cold.root) != nil)
        var absolute = chainEnvironment; absolute["CAPY_WORKLOAD_ROOT"] = "/owned/root"
        let owned = try DrawingWorkloadPlan.configured(environment: absolute)!
        precondition(owned.persistenceRoot.path == "/owned/root")
        let fresh = try DrawingWorkloadPlan(name: "ink", seconds: 600)
        precondition(fresh.chain == nil && fresh.persistenceRoot != fresh.persistenceRoot)
        for (key, value) in [("CAPY_WORKLOAD_CHAIN", nil), ("CAPY_WORKLOAD_OUTPUT", nil), ("CAPY_WORKLOAD_ROOT", nil),
            ("CAPY_WORKLOAD_ROOT", ""), ("CAPY_WORKLOAD_PASS", "0"), ("CAPY_WORKLOAD_PASS", "x"), ("CAPY_WORKLOAD_COLD", "yes"),
            ("CAPY_WORKLOAD_SPACE", "Srgb"), ("CAPY_WORKLOAD_SECONDS", "1801")] as [(String, String?)] {
            var invalid = chainEnvironment; invalid[key] = value
            do { _ = try DrawingWorkloadPlan.configured(environment: invalid); preconditionFailure("Invalid chain configuration accepted: \(key)") }
            catch {}
        }
        do { _ = try DrawingWorkloadPlan(name: "chain", seconds: 600); preconditionFailure("Chain without settings accepted") }
        catch {}
        print("PASS: workload contact termination, lift gaps, bounds, pressure, chain settings, persistence roots and configuration validation")
    }
}
