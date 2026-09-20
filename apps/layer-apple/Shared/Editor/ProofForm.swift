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
