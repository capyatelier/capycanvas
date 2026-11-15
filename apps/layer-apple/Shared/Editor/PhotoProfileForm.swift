import SwiftUI

struct PhotoProfileForm: View {
    @Environment(\.capyProfileCopy) private var copy
    @Environment(\.capyInterfaceLanguage) private var language
    @Environment(\.editorDialogPage) private var page
    @Environment(\.editorPopupStore) private var store
    let preferences: ColorPreferencesStore
    let interpretation: JSON
    let error: String?
    let busy: Bool
    let choose: (JSON?) -> Void
    @State private var selection = JSON()
    @State private var readingProfile = false
    @State private var sourceCaption = ""
    @State private var originalName: String?
    @State private var failure: String?
    private var usable: Bool { !readingProfile && (failure == nil || !selection.isNull) }
    private func useProfile() { choose(selection.isNull ? interpretation["profile"] : selection["profile"]) }
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if page {
                EditorPageHeader(intent: copy["dialog"], title: copy["interpret_title"].string, identifier: "photo-profile",
                    primaryLabel: copy["use_profile"].string, primaryEnabled: usable, primaryIdentifier: "photo-profile-use",
                    cancel: { choose(nil) }, primary: useProfile)
            }
            if page { EditorScrollView { formBody } }
            else { formBody }
        }.modifier(EditorDialogFrame(minWidth: 320, idealWidth: 420, maxWidth: 520)).disabled(busy)
            .onChange(of: [language, interpretation["channels"].string, interpretation["depth"].string], initial: true) { _, _ in
                sourceCaption = NativeTextContext.caption(["type": "photo_source", "channels": interpretation["channels"].raw,
                    "depth": interpretation["depth"].raw], language: language)
            }
            .task(id: language) { describe() }
    }
    private var formBody: some View {
        VStack(alignment: .leading, spacing: 16) {
            if !page { Text(copy["interpret_title"].string).font(.headline) }
            Text(copy["interpret_help"].string)
            SourceProfilePicker(label: copy["interpret_as"].string, preferences: preferences,
                purpose: JSON(["kind": "source", "channels": interpretation["channels"].raw]),
                original: JSON(["name": originalName ?? copy["choose"].string, "profile": interpretation["profile"].raw]),
                selection: $selection, busy: $readingProfile)
            Text(sourceCaption)
                .foregroundStyle(.secondary)
            if let failure { Text(failure).foregroundStyle(.red) }
            if let error { Text(error).foregroundStyle(.red) }
            if !page { HStack {
                Button(copy["common"]["cancel"].string) { choose(nil) }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(copy["use_profile"].string, action: useProfile).keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("photo-profile-use")
                    .disabled(!usable)
            } }
        }.padding(24)
    }
    private func describe() {
        let profile = interpretation["profile"]
        NativeProjectTask.io.async {
            let result = Result { try preferences.describe(profile) }
            DispatchQueue.main.async {
                switch result {
                case .success(let name): store?.query(["type": "profile_name_copy", "name": name.raw]) { originalName = $0.string }
                case .failure(let error): failure = error.localizedDescription
                }
            }
        }
    }
}
