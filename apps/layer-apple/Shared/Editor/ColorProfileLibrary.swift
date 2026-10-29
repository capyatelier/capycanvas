import SwiftUI
import UniformTypeIdentifiers

struct ProfileImportButton: View {
    @Environment(\.capyProfileCopy) private var copy
    let preferences: ColorPreferencesStore
    @Binding var busy: Bool
    let onProfile: (JSON) -> Void
    @State private var choosing = false
    @State private var error: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button(copy[busy ? "reading" : "import"].string) { choosing = true }
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
    @Environment(\.capyProfileCopy) private var copy
    let preferences: ColorPreferencesStore
    @Binding var busy: Bool
    var onLibraryDismiss: (() -> Void)? = nil
    let onProfile: (JSON) -> Void
    @State private var library = false
    var body: some View {
        HStack(alignment: .top) {
            ProfileImportButton(preferences: preferences, busy: $busy, onProfile: onProfile)
            Button(copy["saved_dialog"].string) { library = true }.disabled(busy)
                .accessibilityIdentifier("color-profile-library")
        }.sheet(isPresented: $library, onDismiss: { onLibraryDismiss?() }) {
            ColorProfileLibrary(preferences: preferences) { profile in onProfile(profile); library = false }
                .modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
        }
    }
}

@MainActor final class ProfileEntryCopyCache: ObservableObject {
    private var metadata: [JSON] = []
    private var language: String?
    private var query: String?
    private var cached: [JSON] = []
    func rows(_ entries: [JSON], language: String, query: String = "") -> [JSON] {
        if self.language == language && self.query == query && metadata.count == entries.count
            && zip(metadata, entries).allSatisfy({ SnapshotProjection.equal($0.raw, $1.raw) }) { return cached }
        metadata = entries; self.language = language; self.query = query
        cached = ColorUI.profileEntries(entries, language: language, query: query)
        return cached
    }
}

struct ColorProfileLibrary: View {
    @Environment(\.capyInterfaceLanguage) private var language
    @Environment(\.capyProfileCopy) private var copy
    @Environment(\.capyNativeCopy) private var nativeCopy
    let preferences: ColorPreferencesStore
    var onProfile: ((JSON) -> Void)? = nil
    @Environment(\.dismiss) private var dismiss
    @StateObject private var entryCopy = ProfileEntryCopyCache()
    @State private var metadata: [JSON] = []
    @State private var busy = false
    @State private var error: String?
    @State private var query = ""
    @Environment(\.editorPopupStore) private var store
    private var palette: EditorPalette { EditorPalette(source: store?.state["palette"] ?? JSON()) }
    var body: some View {
        let entries = entryCopy.rows(metadata, language: language, query: query)
        VStack(alignment: .leading, spacing: 16) {
            Text(copy["library_title"].string).font(.headline)
            Text(copy["library_help"].string)
            if metadata.count > 7 || !query.isEmpty {
                TextField(copy["search"].string, text: $query).editorSearchInput().textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("profile-library-search")
            }
            EditorScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(entries, id: \.id) { entry in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(entry["name"].string).font(.headline)
                            if !entry["issue"].isNull { Text(entry["issue"].string).foregroundStyle(.red) }
                            else { Text(entry["details"].string).font(.caption) }
                            HStack {
                                if onProfile != nil {
                                    Button(copy["use_profile"].string) { select(entry["id"].string) }
                                        .disabled(!entry["issue"].isNull).accessibilityIdentifier("profile-use-" + entry["id"].string)
                                }
                                Button(copy["remove"].string, role: .destructive) { remove(entry["id"].string) }
                                    .accessibilityIdentifier("profile-remove-" + entry["id"].string)
                            }
                            Toggle(copy["shown"].string, isOn: Binding(get: { entry["visible"].bool }, set: {
                                reload(showing: (entry["id"].string, $0))
                            })).accessibilityIdentifier("profile-visible-" + entry["id"].string)
                        }
                        EditorLine(color: palette.settingsBorder)
                    }
                    if entries.isEmpty && !busy { Text(copy["empty"].string) }
                }.frame(maxWidth: .infinity, alignment: .leading).disabled(busy)
            }
            if busy { ProgressView(copy["loading"].string) }
            if let error { Text(error).foregroundStyle(.red) }
            HStack {
                ProfileImportButton(preferences: preferences, busy: $busy) { profile in
                    if let onProfile { onProfile(profile) } else { reload() }
                }
                Button(nativeCopy["color"]["refresh"].string) { reload() }.disabled(busy)
                Spacer()
                Button(copy["common"]["done"].string) { dismiss() }.keyboardShortcut(.cancelAction).disabled(busy)
                    .accessibilityIdentifier("profile-library-done")
            }
        }.padding(24).frame(minWidth: 340, idealWidth: 540, maxWidth: 620, minHeight: 300, idealHeight: 560, maxHeight: 720)
            .interactiveDismissDisabled(busy).task { reload() }
    }
    private func reload(removing id: String? = nil, showing: (String, Bool)? = nil) {
        guard !busy else { return }
        busy = true; error = nil
        NativeProjectTask.io.async {
            let result = Result {
                if let id { try preferences.removeProfile(id) }
                if let showing { try preferences.showProfile(showing.0, visible: showing.1) }
                return try preferences.profiles()
            }
            DispatchQueue.main.async {
                busy = false
                switch result {
                case .success(let values): metadata = values
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
