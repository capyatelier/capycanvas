import SwiftUI

/// Source interpretation and repair share the managed ICC picker with export.
struct SourceProfilePicker: View {
    let preferences: ColorPreferencesStore
    let spaces: [JSON]
    @Binding var selection: String
    @Binding var imported: JSON
    @Binding var busy: Bool
    var onImport: () -> Void = {}
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Interpret as", selection: $selection) {
                ForEach(spaces, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
                if !imported["profile"].isNull { Text(imported["name"].string).tag("imported") }
            }.accessibilityIdentifier("photo-profile-space")
            ProfileChooserButtons(preferences: preferences, busy: $busy) { profile in
                imported = profile; selection = "imported"; onImport()
            }
        }.disabled(busy)
    }
}
