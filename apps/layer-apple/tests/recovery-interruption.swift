import Foundation
import Darwin

/// CPU-only production archive/publication helper for the subprocess interruption check.
@main struct RecoveryInterruptionCheck {
    static func require(_ condition: Bool, _ message: String) throws {
        if !condition { throw HostFailure(message: message) }
    }
    static func task(_ platform: UInt32, edits: Int) throws -> NativeProjectTask {
        guard let app = capy_apple_create(platform) else { throw HostFailure(message: "Create owner") }
        defer { capy_apple_destroy(app) }
        for _ in 0..<edits {
            let result = capy_apple_request(app, 0, #"{"type":"invoke","command":"add_layer"}"#)
            if let result { capy_apple_string_free(result) }
            if let error = capy_apple_error(app) { throw HostFailure(message: String(cString: error)) }
        }
        guard let job = capy_apple_project_task(app, 2, nil) else {
            throw HostFailure(message: capy_apple_error(app).map(String.init(cString:)) ?? "Capture failed")
        }
        return NativeProjectTask(job)
    }
    static func main() {
        do { try run() }
        catch { fputs("FAIL: \(error.localizedDescription)\n", stderr); exit(1) }
    }
    static func run() throws {
        setbuf(stdout, nil)
        let args = CommandLine.arguments
        guard args.count == 5, let platform = UInt32(args[2]), let scene = UUID(uuidString: args[4]) else {
            throw HostFailure(message: "Usage: check seed|write|probe|retry|discard|remove|stale platform root scene")
        }
        let mode = args[1], root = URL(fileURLWithPath: args[3], isDirectory: true), files = RecoveryFiles(root: root)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        if mode == "probe" {
            let found = try files.list()
            try require(found.errors.isEmpty && found.records.count <= 1, "Invalid or duplicated recovery record")
            guard let record = found.records.first else { print("NONE"); return }
            try require(record.scene == scene, "Wrong scene")
            let bytes = try Data(contentsOf: files.archive(record)!)
            let old = try Data(contentsOf: root.appendingPathComponent("expected-old.capy"))
            let new = try? Data(contentsOf: root.appendingPathComponent("expected-new.capy"))
            try require(bytes == old || bytes == new, "Published recovery does not match either complete generation")
            print(bytes == old ? "OLD" : "NEW")
        } else if mode == "remove" || mode == "stale" {
            if mode == "stale" {
                let old = try JSONDecoder().decode(RecoveryRecord.self, from: Data(contentsOf: root.appendingPathComponent("old-record.json")))
                try files.remove(old)
            } else if let record = try files.current(scene) { try files.remove(record) }
            print("REMOVED")
        } else if mode == "discard" {
            let record = try files.current(scene)!
            print("READY"); _ = readLine()
            try files.remove(record)
            print("REMOVED")
            while true { pause() }
        } else {
            let job = try task(platform, edits: mode == "seed" ? 1 : 2)
            let expected = root.appendingPathComponent(mode == "seed" ? "expected-old.capy" : "expected-new.capy")
            try job.write(to: expected)
            if mode == "archive" {
                // Hold an actual partial project in the production atomic
                // writer. Foundation's replacement directory is outside the
                // recovery folder, and small writes finish too fast to sample.
                let folder = root.appendingPathComponent("recovery/\(scene.uuidString)")
                let destination = folder.appendingPathComponent("\(UUID()).capy")
                let bytes = try Data(contentsOf: expected)
                try ProjectFileIO.coordinate(destination, writing: true) { location in
                    try ProjectFileIO.atomicWrite(to: location) { descriptor in
                        let count = bytes.count / 2
                        try require(bytes.withUnsafeBytes { Darwin.write(descriptor, $0.baseAddress, count) } == count,
                            "Stage an incomplete project")
                        print("READY"); _ = readLine()
                        while true { pause() }
                    }
                }
            } else if mode == "write" {
                print("READY"); _ = readLine()
                for generation in 0..<2000 { try files.write(job, scene: scene, title: "Candidate \(generation)") }
                print("FINISHED")
                while true { pause() }
            } else {
                let record = try files.write(job, scene: scene, title: mode == "seed" ? "Baseline" : "Retry")!
                if mode == "seed" {
                    try JSONEncoder().encode(record).write(to: root.appendingPathComponent("old-record.json"))
                }
                print("SAVED")
            }
        }
    }
}
