import SwiftUI
import UniformTypeIdentifiers

struct ProfileImportButton: View {
    let preferences: ColorPreferencesStore
    @Binding var busy: Bool
    let onProfile: (JSON) -> Void
    @State private var choosing = false
    @State private var error: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button(busy ? "Reading profile…" : "Import ICC Profile…") { choosing = true }
                .disabled(busy).accessibilityIdentifier("source-profile-import")
            if let error { Text(error).foregroundStyle(.red) }
        }.fileImporter(isPresented: $choosing, allowedContentTypes: [UTType(filenameExtension: "icc") ?? .data, UTType(filenameExtension: "icm") ?? .data]) { result in
            switch result {
            case .success(let url):
                busy = true; error = nil
                NativeProjectTask.io.async {
                    let result = Result { try preferences.importProfile(url) }
                    DispatchQueue.main.async {
                        busy = false
                        switch result {
                        case .success(let profile): onProfile(profile)
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

struct ProfileChooserButtons: View {
    let preferences: ColorPreferencesStore
    @Binding var busy: Bool
    let onProfile: (JSON) -> Void
    @State private var library = false
    var body: some View {
        HStack(alignment: .top) {
            ProfileImportButton(preferences: preferences, busy: $busy, onProfile: onProfile)
            Button("Saved Profiles…") { library = true }.disabled(busy)
                .accessibilityIdentifier("color-profile-library")
        }.sheet(isPresented: $library) {
            ColorProfileLibrary(preferences: preferences) { profile in onProfile(profile); library = false }
                .modifier(EditorPopupPresentation())
        }
    }
}

struct ColorProfileLibrary: View {
    let preferences: ColorPreferencesStore
    var onProfile: ((JSON) -> Void)? = nil
    @Environment(\.dismiss) private var dismiss
    @State private var entries: [JSON] = []
    @State private var busy = false
    @State private var error: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Color Profile Library").font(.headline)
            Text("Imported profiles are saved as exact copies. Removing one leaves original files and profiles embedded in drawings or export presets intact.")
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(entries, id: \.id) { entry in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(entry["name"].string).font(.headline)
                            if !entry["issue"].isNull { Text(entry["issue"].string).foregroundStyle(.red) }
                            else { Text("\(entry["channels"].string) · \(ByteCountFormatter.string(fromByteCount: Int64(entry["bytes"].uint), countStyle: .file))").font(.caption) }
                            HStack {
                                if onProfile != nil {
                                    Button("Use Profile") { select(entry["id"].string) }
                                        .disabled(!entry["issue"].isNull).accessibilityIdentifier("profile-use-" + entry["id"].string)
                                }
                                Button("Remove", role: .destructive) { remove(entry["id"].string) }
                                    .accessibilityIdentifier("profile-remove-" + entry["id"].string)
                            }
                        }
                        Divider()
                    }
                    if entries.isEmpty && !busy { Text("No imported profiles") }
                }.frame(maxWidth: .infinity, alignment: .leading).disabled(busy)
            }
            if busy { ProgressView("Reading color profiles…") }
            if let error { Text(error).foregroundStyle(.red) }
            HStack {
                ProfileImportButton(preferences: preferences, busy: $busy) { profile in
                    if let onProfile { onProfile(profile) } else { reload() }
                }
                Button("Refresh") { reload() }.disabled(busy)
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction).disabled(busy)
                    .accessibilityIdentifier("profile-library-done")
            }
        }.padding(24).frame(minWidth: 340, idealWidth: 540, maxWidth: 620, minHeight: 300, idealHeight: 560, maxHeight: 720)
            .interactiveDismissDisabled(busy).task { reload() }
    }
    private func reload(removing id: String? = nil) {
        guard !busy else { return }
        busy = true; error = nil
        NativeProjectTask.io.async {
            let result = Result {
                if let id { try preferences.removeProfile(id) }
                return try preferences.profiles()
            }
            DispatchQueue.main.async {
                busy = false
                switch result {
                case .success(let values): entries = values
                case .failure(let failure): error = failure.localizedDescription
                }
            }
        }
    }
    private func remove(_ id: String) { reload(removing: id) }
    private func select(_ id: String) {
        guard !busy else { return }
        busy = true; error = nil
        NativeProjectTask.io.async {
            let result = Result { try preferences.profile(id) }
            DispatchQueue.main.async {
                busy = false
                switch result {
                case .success(let profile): onProfile?(profile)
                case .failure(let failure): error = failure.localizedDescription
                }
            }
        }
    }
}

private extension JSON { var id: String { self["id"].string } }
