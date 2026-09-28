import SwiftUI

struct ProofIndicator: View {
    @ObservedObject var model: ProofController
    let palette: EditorPalette
    var body: some View {
        if !model.status.isEmpty {
            Text(model.status).lineLimit(1).truncationMode(.middle)
                .padding(.horizontal, 10).padding(.vertical, 3)
                .glassSurface(SquircleShape.tile, fill: palette.chromeSurface)
                .help(model.error ?? model.status).allowsHitTesting(false)
                .accessibilityIdentifier("proof-status")
        }
    }
}

struct ToneStatusLabel: View {
    @ObservedObject var store: EditorStore
    let palette: EditorPalette
    private var text: String {
        let status = store.snapshot["display_status"]
        guard status["hdr"].bool, !status["hdr_output"].bool else { return "" }
        return !status["error"].isNull ? "SDR preview unavailable" : status["retained"].bool ? "" : "Preparing SDR…"
    }
    var body: some View {
        if !text.isEmpty {
            Text(text).lineLimit(1).padding(.horizontal, 10).padding(.vertical, 3)
                .glassSurface(SquircleShape.tile, fill: palette.chromeSurface)
                .allowsHitTesting(false).accessibilityIdentifier("tone-status")
        }
    }
}

struct ScreenStatus: View {
    @ObservedObject var store: EditorStore
    let palette: EditorPalette
    @State private var open = false
    private var screen: JSON { store.state["screen"] }
    private var warning: Color { Color(hex: store.state["theme"].string == "dark" ? "#e5a50a" : "#9c5700") }
    var body: some View {
        let chip = screen["chip"]
        if !chip.isNull {
            Button { open.toggle() } label: {
                HStack(spacing: 4) {
                    if chip["warning"].bool { SharedIcon(name: "warning", size: 16).foregroundStyle(warning) }
                    Text(chip["label"].string).lineLimit(1)
                }.padding(.horizontal, 10).padding(.vertical, 3)
            }.buttonStyle(ReadoutButtonStyle(palette: palette)).focusable(false).help("Screen details")
                .accessibilityIdentifier("screen-status")
                .editorPopover(isPresented: Binding(get: { open && !screen["details"].isNull }, set: { open = $0 })) { details }
        }
    }
    private var details: some View {
        let details = screen["details"]
        return VStack(alignment: .leading, spacing: 6) {
            Text(details["title"].string).font(.callout).opacity(0.7)
            Text(details["headline"].string).fontWeight(.semibold)
                .foregroundStyle(details["warning"].bool ? warning : palette["text"])
                .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("screen-details-headline")
            if !details["body"].isNull {
                Text(details["body"].string).fixedSize(horizontal: false, vertical: true)
            }
            if !details["show_clipped"].isNull {
                Toggle("Highlight these colors", isOn: Binding(get: { details["show_clipped"].bool },
                    set: { store.dispatch(["type": "show_clipped_colors", "visible": $0]) }))
                    .accessibilityIdentifier("screen-show-clipped")
            }
        }.padding(.vertical, 12).padding(.horizontal, 14).frame(width: 340, alignment: .leading)
            .accessibilityElement(children: .contain).accessibilityIdentifier("screen-details")
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
            }.modifier(GlassRegistration(shape: SquircleShape.tile)).onHover { hovering = $0 }
        }
    }
}
