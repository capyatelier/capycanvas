import SwiftUI

struct ProfileMenuRequest: Equatable {
    let purpose: JSON
    let document: JSON
    let current: JSON
    let language: String
    let revision: Int
    static func == (left: Self, right: Self) -> Bool {
        left.language == right.language && left.revision == right.revision && MainActor.assumeIsolated {
            SnapshotProjection.equal(left.purpose.raw, right.purpose.raw) && SnapshotProjection.equal(left.document.raw, right.document.raw)
                && SnapshotProjection.equal(left.current.raw, right.current.raw)
        }
    }
}

@MainActor final class ProfileMenuModel: ObservableObject {
    @Published private(set) var inventory = JSON()
    @Published private(set) var standards: [JSON] = []
    @Published private(set) var saved: [JSON] = []
    @Published var error: String?
    func load(_ request: ProfileMenuRequest, preferences: ColorPreferencesStore) {
        inventory = JSON()
        NativeProjectTask.io.async { [weak self] in
            let result = Result { try preferences.profileMenu(purpose: request.purpose, document: request.document, current: request.current) }
            DispatchQueue.main.async {
                guard let self else { return }
                switch result {
                case .success(let reply):
                    let rows = ColorUI.profileEntries(reply.saved, language: request.language)
                    self.saved = reply.inventory["saved"].array.compactMap { index in
                        rows.first { $0["id"].string == reply.saved[Int(index.uint)]["id"].string }
                    }
                    self.standards = reply.standards; self.inventory = reply.inventory; self.error = nil
                case .failure(let failure): self.error = failure.localizedDescription
                }
            }
        }
    }
}

struct ProfileMenuItems: View {
    @Environment(\.capyProfileCopy) private var copy
    @ObservedObject var menu: ProfileMenuModel
    let preferences: ColorPreferencesStore
    let document: JSON
    let current: JSON
    @Binding var busy: Bool
    let onChoose: (JSON) -> Void
    var body: some View {
        if menu.inventory.isNull && menu.error == nil { Text(copy["loading"].string) }
        if menu.inventory["document"].bool { Section(copy["document"].string) { item(document) } }
        if menu.inventory["current"].bool { Section(copy["current"].string) { item(current) } }
        if !menu.saved.isEmpty {
            Section(copy["saved"].string) {
                ForEach(menu.saved, id: \.stableKey) { entry in Button(entry["name"].string) { read(entry["id"].string) } }
            }
        }
        if menu.inventory["standard"].bool {
            Section(copy["standard"].string) { ForEach(menu.standards, id: \.stableKey) { item($0) } }
        }
    }
    private func item(_ profile: JSON) -> some View { Button(profile["name"].string) { onChoose(profile) } }
    private func read(_ id: String) {
        busy = true
        NativeProjectTask.io.async {
            let result = Result { try preferences.profile(id) }
            DispatchQueue.main.async {
                busy = false
                switch result {
                case .success(let profile): onChoose(profile)
                case .failure(let failure): menu.error = failure.localizedDescription
                }
            }
        }
    }
}

struct SourceProfilePicker: View {
    @Environment(\.capyInterfaceLanguage) private var language
    let label: String
    let preferences: ColorPreferencesStore
    let purpose: JSON
    let original: JSON
    @Binding var selection: JSON
    @Binding var busy: Bool
    @StateObject private var menu = ProfileMenuModel()
    @State private var revision = 0
    private var current: JSON { selection.isNull ? original : selection }
    private var request: ProfileMenuRequest {
        ProfileMenuRequest(purpose: purpose, document: JSON(), current: current["profile"], language: language, revision: revision)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            #if os(iOS)
            Text(label).font(.subheadline).foregroundStyle(.secondary)
            chooser
            #else
            LabeledContent(label) { chooser }
            #endif
            ProfileChooserButtons(preferences: preferences, busy: $busy, onLibraryDismiss: { revision += 1 }) { selection = $0 }
            if let error = menu.error { Text(error).foregroundStyle(.red) }
        }.disabled(busy)
            .task(id: request) { menu.load(request, preferences: preferences) }
    }
    private var chooser: some View {
        Menu {
            ProfileMenuItems(menu: menu, preferences: preferences, document: JSON(), current: current, busy: $busy) { selection = $0 }
        } label: { Text(current["name"].string) }
            .accessibilityIdentifier("photo-profile-space")
    }
}
