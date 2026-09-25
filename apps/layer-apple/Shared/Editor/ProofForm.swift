import SwiftUI

struct ProofIndicator: View {
    @ObservedObject var model: ProofController
    let palette: EditorPalette
    var body: some View {
        if !model.status.isEmpty {
            Text(model.status).lineLimit(1).truncationMode(.middle)
                .padding(.horizontal, 10).padding(.vertical, 3)
                .background(palette.chromeSurface, in: SquircleShape.tile)
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
                Text(status["label"].string).lineLimit(1).padding(.horizontal, 10).padding(.vertical, 3)
            }.buttonStyle(ReadoutButtonStyle(palette: palette)).accessibilityIdentifier("hdr-status").help("Display Details")
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

private struct ReadoutButtonStyle: ButtonStyle {
    let palette: EditorPalette
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, palette: palette) }
    private struct Face: View {
        let configuration: Configuration
        let palette: EditorPalette
        @State private var hovering = false
        var body: some View {
            configuration.label.background {
                ZStack {
                    SquircleShape.tile.fill(palette.chromeSurface)
                    if configuration.isPressed || hovering {
                        SquircleShape.tile.fill(palette["text"].opacity(configuration.isPressed ? 0.16 : 0.10))
                    }
                }
            }.onHover { hovering = $0 }
        }
    }
}
