import SwiftUI

struct PhotoProfileForm: View {
    let interpretation: JSON
    let spaces: [JSON]
    let error: String?
    let busy: Bool
    let choose: (JSON?) -> Void
    @State private var space = "Srgb"
    @State private var imported = JSON()
    @State private var readingProfile = false
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Choose image interpretation").font(.headline)
            Text("This image has no declared color profile. Choose how to interpret its stored values. The original numbers will be retained.")
            SourceProfilePicker(spaces: spaces, selection: $space, imported: $imported, busy: $readingProfile)
            Text("Source: \(interpretation["channels"].string) · \(interpretation["depth"].string == "U16" ? "16" : "8")-bit")
                .foregroundStyle(.secondary)
            if let error { Text(error).foregroundStyle(.red) }
            HStack {
                Button("Cancel") { choose(nil) }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Use Profile") { choose(space == "imported" ? imported["profile"] : JSON(["Builtin": space])) }.keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("photo-profile-use")
                    .disabled(readingProfile)
            }
        }.padding(24).frame(minWidth: 320, idealWidth: 420, maxWidth: 520).disabled(busy)
    }
}
