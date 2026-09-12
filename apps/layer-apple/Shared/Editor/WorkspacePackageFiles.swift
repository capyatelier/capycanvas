import SwiftUI
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#else
import UIKit
#endif

enum WorkspacePackageKind: String, CaseIterable {
    case workspaceBackup = "workspace_backup", template, toolbar
    var fileExtension: String {
        switch self { case .workspaceBackup: "capyworkspace"; case .template: "capytemplate"; case .toolbar: "capytoolbar" }
    }
    var contentType: UTType { UTType(exportedAs: "art.capycanvas." + fileExtension, conformingTo: .data) }
    static func forURL(_ url: URL) -> Self? { allCases.first { $0.fileExtension == url.pathExtension.lowercased() } }
}

/// Native URL delivery only. Rust owns package validation and serialization;
/// providers and filesystem work run on the file queue, outside the editor.
@MainActor final class WorkspacePackageFiles: ObservableObject {
    struct Dialogs {
        var open: (UTType, @escaping (URL?) -> Void) -> Void
        var save: (String, UTType, @escaping (URL?) -> Void) -> Void
        var export: ((URL, @escaping (URL?) -> Void) -> Void)?
    }
    struct Picker: Identifiable {
        let id = UUID()
        let kind: WorkspacePackageKind?
        let export: URL?
    }
    @Published var picker: Picker?
    private var pickerCompletion: ((URL?) -> Void)?
    private let dialogs: Dialogs?
    private static let io = DispatchQueue(label: "art.capycanvas.workspace-packages", qos: .userInitiated)
    init(dialogs: Dialogs? = nil) { self.dialogs = dialogs }
    private func work<T: Sendable>(_ body: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            Self.io.async { do { continuation.resume(returning: try body()) } catch { continuation.resume(throwing: error) } }
        }
    }
    func read(kind: WorkspacePackageKind, url: URL? = nil) async throws -> String? {
        let picked: URL?
        if let url { picked = url } else { picked = await chooseOpen(kind) }
        guard let source = picked else { return nil }
        return try await work {
            var result: String?
            try ProjectFileIO.coordinate(source, writing: false) { source in
                let file = try FileHandle(forReadingFrom: source)
                defer { try? file.close() }
                let maximum = 128 * 1024 * 1024
                let data = try file.read(upToCount: maximum + 1) ?? Data()
                guard data.count <= maximum else { throw HostFailure(message: "The workspace package is too large") }
                guard let text = String(data: data, encoding: .utf8) else { throw HostFailure(message: "The workspace package is not valid UTF-8") }
                result = text
            }
            return result
        }
    }
    func export(_ value: JSON) async throws -> Bool {
        let staging = try await work {
            let suffix = value["extension"].string
            let url = try ProjectFileIO.stagingURL(title: value["name"].string + "." + suffix, extension: suffix)
            do { try Self.write(Data(value["text"].string.utf8), to: url); return url }
            catch { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()); throw error }
        }
        defer { Self.io.async { try? FileManager.default.removeItem(at: staging.deletingLastPathComponent()) } }
        let type = WorkspacePackageKind.forURL(staging)?.contentType ?? .data
        return try await deliver(staging, type: type)
    }
    func exportDatabase(_ library: WorkspaceLibrary) async throws -> Bool {
        let staging = try await work { try ProjectFileIO.stagingURL(title: "capycanvas-workspaces", extension: "sqlite3") }
        defer { Self.io.async { try? FileManager.default.removeItem(at: staging.deletingLastPathComponent()) } }
        try await library.backup(to: staging)
        return try await deliver(staging, type: UTType(filenameExtension: "sqlite3") ?? .data)
    }
    private func deliver(_ staging: URL, type: UTType) async throws -> Bool {
        #if os(iOS)
        if dialogs?.export == nil && dialogs == nil {
            let destination: URL? = await withCheckedContinuation { continuation in
                pickerCompletion = { continuation.resume(returning: $0) }; picker = Picker(kind: nil, export: staging)
            }
            return destination != nil
        }
        #endif
        if let export = dialogs?.export {
            return await withCheckedContinuation { continuation in export(staging) { continuation.resume(returning: $0 != nil) } }
        }
        guard let destination = await chooseSave(staging.lastPathComponent, type: type) else { return false }
        try await work {
            try ProjectFileIO.coordinate(destination, writing: true) { destination in
                let input = try FileHandle(forReadingFrom: staging)
                defer { try? input.close() }
                try ProjectFileIO.atomicWrite(to: destination) { descriptor in
                    let output = FileHandle(fileDescriptor: descriptor, closeOnDealloc: false)
                    while let data = try input.read(upToCount: 1_048_576), !data.isEmpty { try output.write(contentsOf: data) }
                }
            }
        }
        return true
    }
    private nonisolated static func write(_ data: Data, to url: URL) throws {
        try ProjectFileIO.atomicWrite(to: url) { descriptor in
            try FileHandle(fileDescriptor: descriptor, closeOnDealloc: false).write(contentsOf: data)
        }
    }
    private func chooseOpen(_ kind: WorkspacePackageKind) async -> URL? {
        await withCheckedContinuation { continuation in
            if let dialogs { dialogs.open(kind.contentType) { continuation.resume(returning: $0) }; return }
            #if os(macOS)
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [kind.contentType]; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
            panel.begin { continuation.resume(returning: $0 == .OK ? panel.url : nil) }
            #else
            pickerCompletion = { continuation.resume(returning: $0) }; picker = Picker(kind: kind, export: nil)
            #endif
        }
    }
    private func chooseSave(_ name: String, type: UTType) async -> URL? {
        await withCheckedContinuation { continuation in
            if let dialogs { dialogs.save(name, type) { continuation.resume(returning: $0) }; return }
            #if os(macOS)
            let panel = NSSavePanel()
            panel.allowedContentTypes = [type]; panel.canCreateDirectories = true; panel.nameFieldStringValue = name
            panel.begin { continuation.resume(returning: $0 == .OK ? panel.url : nil) }
            #else
            continuation.resume(returning: nil)
            #endif
        }
    }
    func picked(_ url: URL?) {
        let completion = pickerCompletion; pickerCompletion = nil; picker = nil
        completion?(url)
    }
}

struct WorkspacePackagePicker: ViewModifier {
    @ObservedObject var files: WorkspacePackageFiles
    func body(content: Content) -> some View {
        #if os(iOS)
        content.sheet(item: $files.picker, onDismiss: { files.picked(nil) }) { picker in
            WorkspaceDocumentPicker(picker: picker) { files.picked($0) }.ignoresSafeArea()
        }
        #else
        content
        #endif
    }
}
#if os(iOS)
private struct WorkspaceDocumentPicker: UIViewControllerRepresentable {
    let picker: WorkspacePackageFiles.Picker
    let completion: (URL?) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(completion) }
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let controller = picker.export.map { UIDocumentPickerViewController(forExporting: [$0], asCopy: false) }
            ?? UIDocumentPickerViewController(forOpeningContentTypes: [picker.kind?.contentType ?? .data], asCopy: false)
        controller.allowsMultipleSelection = false; controller.delegate = context.coordinator
        return controller
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let completion: (URL?) -> Void
        init(_ completion: @escaping (URL?) -> Void) { self.completion = completion }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { completion(urls.first) }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { completion(nil) }
    }
}
#endif
