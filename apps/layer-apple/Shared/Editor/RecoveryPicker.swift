import SwiftUI

struct RecoveryPicker: View {
    @ObservedObject var recovery: ArtworkRecovery
    @State private var discarding: RecoveryRecord?
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Recovered Drawings").font(.title2).bold()
                Spacer()
                Button("Done") { recovery.presented = false }.keyboardShortcut(.cancelAction)
            }
            Text("Open a recovery copy to continue editing, then save it to keep your changes.")
            if recovery.records.isEmpty { Text("No recovered drawings.").foregroundStyle(.secondary) }
            List(recovery.records) { record in
                HStack {
                    VStack(alignment: .leading) {
                        Text(record.title).bold()
                        Text(record.modified, format: .dateTime.year().month().day().hour().minute())
                    }
                    Spacer()
                    Button("Open") { recovery.choose(record) }.buttonStyle(.borderless)
                        .disabled(!recovery.canOpen)
                        .accessibilityIdentifier("open-recovery-\(record.id)")
                    Button("Discard", role: .destructive) { discarding = record }.buttonStyle(.borderless)
                }.padding(.vertical, 4)
            }.frame(minHeight: 160)
            if let error = recovery.error { Text(error).foregroundStyle(.red) }
        }.padding(24).frame(minWidth: 450, idealWidth: 600, minHeight: 300, idealHeight: 420)
            .confirmationDialog("Discard this recovered drawing?", isPresented: Binding(
                get: { discarding != nil }, set: { if !$0 { discarding = nil } })) {
                Button("Discard", role: .destructive) {
                    if let record = discarding { recovery.discard(record) }; discarding = nil
                }
            }
    }
}

struct RecoveryPresentation: ViewModifier {
    @ObservedObject var recovery: ArtworkRecovery
    func body(content: Content) -> some View {
        content.sheet(isPresented: $recovery.presented, onDismiss: recovery.dismissed) { RecoveryPicker(recovery: recovery) }
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
                if let error = recovery.error {
                    HStack {
                        Text(error)
                        Button("Retry") { recovery.refresh(); recovery.flush { _ in } }
                        Button("Recovered Drawings…") { recovery.refresh(); recovery.presented = true }
                    }.padding(12).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8)).padding()
                }
            }
    }
}
