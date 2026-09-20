import SwiftUI

struct ProofIndicator: View {
    @ObservedObject var model: ProofController
    let palette: EditorPalette
    var body: some View {
        if !model.status.isEmpty {
            Text(model.status).lineLimit(1).truncationMode(.middle)
                .padding(.horizontal, 8).padding(.vertical, 4)
                .background(palette["bg"], in: Capsule())
                .help(model.error ?? model.status).allowsHitTesting(false)
                .accessibilityIdentifier("proof-status")
        }
    }
}

struct HDRDisplayIndicator: View {
    @ObservedObject var store: EditorStore
    let palette: EditorPalette
    @State private var details = false
    private var status: JSON { store.snapshot["display_status"] }
    var body: some View {
        if status["hdr"].bool {
            Button { details = true } label: {
                Text(status["label"].string).lineLimit(1).padding(.horizontal, 8).padding(.vertical, 4)
                    .background(palette["bg"], in: Capsule())
            }.buttonStyle(.plain).accessibilityIdentifier("hdr-status").help("Display Details")
                .popover(isPresented: $details) {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Display Details").font(.headline)
                        Text(status["hdr_output"].bool ? "Showing HDR. Brightness depends on your display and system settings." :
                            "Showing the saved SDR appearance or selected proof. The HDR master is preserved.")
                        if !status["error"].isNull { Text(status["error"].string).foregroundStyle(.red) }
                        Text("Artwork reference white: 203 cd/m².")
                        Text(String(format: "Current display headroom: %.2f×", status["headroom"].number))
                        Button("Close") { details = false }.keyboardShortcut(.cancelAction)
                    }.padding(20).frame(width: 320).accessibilityIdentifier("display-details")
                }
        }
    }
}

struct ProofPresentation: ViewModifier {
    @ObservedObject var model: ProofController
    @Environment(\.scenePhase) private var phase
    func body(content: Content) -> some View {
        content
        .onAppear { model.setPaused(phase == .background) }
        .onChange(of: phase) { _, phase in model.setPaused(phase == .background) }
        .onDisappear { model.setPaused(true) }
    }
}
