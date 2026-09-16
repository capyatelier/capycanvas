import SwiftUI

struct PhotoProfileForm: View {
    let interpretation: JSON
    let spaces: [JSON]
    let error: String?
    let busy: Bool
    let choose: (JSON?) -> Void
    @State private var space = "Srgb"
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Choose image interpretation").font(.headline)
            Text("This image has no declared color profile. Choose how to interpret its stored values. The original numbers will be retained.")
            Picker("Interpret as", selection: $space) {
                ForEach(spaces, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
            }.accessibilityIdentifier("photo-profile-space")
            Text("Source: \(interpretation["channels"].string) · \(interpretation["depth"].string == "U16" ? "16" : "8")-bit")
                .foregroundStyle(.secondary)
            if let error { Text(error).foregroundStyle(.red) }
            HStack {
                Button("Cancel") { choose(nil) }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Use Profile") { choose(JSON(["Builtin": space])) }.keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("photo-profile-use")
            }
        }.padding(24).frame(minWidth: 320, idealWidth: 420, maxWidth: 520).disabled(busy)
    }
}
