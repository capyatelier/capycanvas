import SwiftUI
import UniformTypeIdentifiers

enum ColorProfiles {
    /// Coordinated, bounded reads and shared ICC inspection happen on the file worker.
    static func read(_ url: URL) throws -> JSON {
        try ProjectFileIO.coordinate(url, writing: false) { source in
            let file = try FileHandle(forReadingFrom: source)
            defer { try? file.close() }
            guard let value = capy_color_profile_read(file.fileDescriptor) else {
                throw HostFailure(message: "The color profile is unavailable")
            }
            defer { capy_apple_string_free(value) }
            let profile = try JSON.decode(String(cString: value))
            if !profile["error"].isNull { throw HostFailure(message: profile["error"].string) }
            return profile
        }
    }
}

/// Shared by missing-profile interpretation and repair of retained photos.
struct SourceProfilePicker: View {
    let spaces: [JSON]
    @Binding var selection: String
    @Binding var imported: JSON
    @Binding var busy: Bool
    var onImport: () -> Void = {}
    @State private var choosing = false
    @State private var error: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Interpret as", selection: $selection) {
                ForEach(spaces, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
                if !imported["profile"].isNull { Text(imported["name"].string).tag("imported") }
            }.accessibilityIdentifier("photo-profile-space")
            Button(busy ? "Reading profile…" : "Import ICC Profile…") { choosing = true }
                .accessibilityIdentifier("source-profile-import")
            if let error { Text(error).foregroundStyle(.red) }
        }.disabled(busy)
            .fileImporter(isPresented: $choosing, allowedContentTypes: [UTType(filenameExtension: "icc") ?? .data, UTType(filenameExtension: "icm") ?? .data]) { result in
                switch result {
                case .success(let url):
                    busy = true; error = nil
                    NativeProjectTask.io.async {
                        let result = Result { try ColorProfiles.read(url) }
                        DispatchQueue.main.async {
                            busy = false
                            switch result {
                            case .success(let profile): imported = profile; selection = "imported"; onImport()
                            case .failure(let failure): error = failure.localizedDescription
                            }
                        }
                    }
                case .failure(let failure):
                    let native = failure as NSError
                    if native.domain != NSCocoaErrorDomain || native.code != NSUserCancelledError { error = failure.localizedDescription }
                }
            }
    }
}
