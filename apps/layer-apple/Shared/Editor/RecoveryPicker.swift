import SwiftUI

struct RecoveryPresentation: ViewModifier {
    @ObservedObject var recovery: ArtworkRecovery
    func body(content: Content) -> some View {
        content.disabled(recovery.restoring).allowsHitTesting(!recovery.restoring)
            #if DEBUG
            .overlay(alignment: .topLeading) {
                if ProcessInfo.processInfo.environment["CAPY_PERSISTENCE_PROBE"] == "1" {
                    Text(recovery.hasCurrentCopy ? "Recovery ready" : "Recovery pending")
                        .foregroundStyle(.clear).frame(width: 1, height: 1)
                        .accessibilityIdentifier("recovery-status")
                }
            }
            #endif
            .overlay(alignment: .bottom) {
                if let error = recovery.error ?? recovery.restoreError {
                    HStack {
                        Text(error).textSelection(.enabled)
                        Button(recovery.copy["retry"].string) { recovery.retry() }
                            .disabled(recovery.restoring).accessibilityIdentifier("recovery-retry")
                        if recovery.canContinue {
                            Button(recovery.copy["later"].string) { recovery.later() }.accessibilityIdentifier("recovery-later")
                        }
                    }.padding(12).modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 8))).padding()
                }
            }
    }
}
