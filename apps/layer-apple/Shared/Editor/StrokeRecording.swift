import SwiftUI
import UniformTypeIdentifiers

@MainActor final class StrokeRecording: ObservableObject {
    private weak var store: EditorStore?
    @Published private(set) var status = JSON()
    @Published private(set) var busy = false
    @Published var export: BinaryFileDocument?
    @Published var error: String?
    private var polling: Task<Void, Never>?
    init(store: EditorStore) { self.store = store }
    var label: String { status["label"].isNull ? "Start stroke recording" : status["label"].string }

    func refresh() { request(nil) }
    func activate() {
        guard !busy else { return }
        if status["recording"].bool { request("stop") }
        else if status["ready"].bool { save() }
        else { request("start") }
    }
    private func request(_ action: String?) {
        store?.query(["type": "stroke_recording", "action": action ?? NSNull()]) { [weak self] result in
            guard let self, !result.isNull else { return }
            let wasRecording = status["recording"].bool
            status = result
            if result["recording"].bool { poll() } else { polling?.cancel(); polling = nil }
            if wasRecording && !result["recording"].bool && result["ready"].bool { save() }
        }
    }
    private func poll() {
        guard polling == nil else { return }
        polling = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(200))
                guard !Task.isCancelled, let self else { return }
                request(nil)
                if !status["recording"].bool { return }
            }
        }
    }
    private func save() {
        guard let native = store?.native, !busy else { return }
        busy = true
        native.strokeRecordingData { result in
            let compressed = result.flatMap { raw in Result { try Self.compress(raw) } }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                busy = false
                switch compressed {
                case .success(let data): export = BinaryFileDocument(data: data)
                case .failure(let failure): error = failure.localizedDescription
                }
            }
        }
    }
    nonisolated private static func compress(_ raw: Data) throws -> Data {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("capy-strokes-" + UUID().uuidString)
        guard FileManager.default.createFile(atPath: url.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
            throw HostFailure(message: "Could not prepare the stroke recording")
        }
        defer { try? FileManager.default.removeItem(at: url) }
        let file = try FileHandle(forWritingTo: url)
        let failure = raw.withUnsafeBytes { buffer in
            capy_stroke_recording_write(buffer.bindMemory(to: UInt8.self).baseAddress, raw.count, file.fileDescriptor)
        }
        try file.close()
        if let failure {
            defer { capy_apple_string_free(failure) }
            throw HostFailure(message: (try? JSON.decode(String(cString: failure)))?["error"].string ?? "Could not save the stroke recording")
        }
        return try Data(contentsOf: url)
    }
    func exported(_ result: Result<URL, Error>) {
        export = nil
        switch result {
        case .success: request("saved")
        case .failure(let failure):
            let e = failure as NSError
            if e.domain != NSCocoaErrorDomain || e.code != NSUserCancelledError { error = failure.localizedDescription }
        }
    }
}

struct StrokeRecordingFiles: ViewModifier {
    @ObservedObject var recording: StrokeRecording
    func body(content: Content) -> some View {
        content
            .fileExporter(isPresented: Binding(get: { recording.export != nil }, set: { if !$0 { recording.export = nil } }),
                document: recording.export, contentType: UTType(filenameExtension: "capystrokes") ?? .data,
                defaultFilename: "stroke-recording.capystrokes") { recording.exported($0) }
            .alert("Stroke recording", isPresented: Binding(get: { recording.error != nil }, set: { if !$0 { recording.error = nil } })) {
                Button("OK", role: .cancel) { recording.error = nil }
            } message: { Text(recording.error ?? "") }
    }
}
