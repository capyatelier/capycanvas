import SwiftUI
import UniformTypeIdentifiers

/// Native transport/hit coordinates only. Shared queries and task capture own
/// layer insertion rules and the camera-to-document conversion.
struct PhotoDropTarget: ViewModifier {
    @ObservedObject var store: EditorStore
    var row: UInt64?
    @Environment(\.displayScale) private var scale
    @StateObject private var feedback = PhotoDropFeedback()
    @State private var height: CGFloat = 0
    func body(content: Content) -> some View {
        content.onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height = $0 }
            .onDrop(of: [UTType.fileURL] + UTType.capyPhotoTypes,
                delegate: PhotoDropDelegate(store: store, row: row, height: height, scale: scale, feedback: feedback))
            .overlay {
                if row != nil, let position = feedback.position {
                    if position == "into" {
                        Rectangle().stroke(EditorPalette.sharedAccent, lineWidth: 2).allowsHitTesting(false)
                    } else {
                        VStack(spacing: 0) {
                            if position == "below" { Spacer(minLength: 0) }
                            Rectangle().fill(EditorPalette.sharedAccent).frame(height: 2)
                            if position == "above" { Spacer(minLength: 0) }
                        }.allowsHitTesting(false)
                    }
                }
            }
            .onChange(of: store.state["revision"].uint) { _, _ in feedback.clear() }
            .onDisappear { feedback.clear() }
    }
}

@MainActor private final class PhotoDropFeedback: ObservableObject {
    @Published private var hint: JSON?
    var position: String? { hint.flatMap { $0["position"].isNull ? nil : $0["position"].string } }
    // A pending query must not reject a stationary native drop before the
    // owner replies. The placement task revalidates the actual release target.
    var allowed: Bool { hint == nil || position != nil }
    private var generation: UInt64 = 0
    private var fraction: Double?
    func clear() { generation &+= 1; hint = nil; fraction = nil }
    func update(_ store: EditorStore, row: UInt64, fraction: Double) {
        guard self.fraction != fraction else { return }
        self.fraction = fraction; hint = nil
        generation &+= 1
        let request = generation
        store.query(["type": "image_layer_drop", "target": row, "fraction": fraction]) { [weak self] value in
            guard let self, generation == request else { return }
            hint = value
        }
    }
}

private struct PhotoDropDelegate: DropDelegate {
    let store: EditorStore
    let row: UInt64?
    let height: CGFloat
    let scale: CGFloat
    let feedback: PhotoDropFeedback
    private var types: [UTType] { [UTType.fileURL] + UTType.capyPhotoTypes }
    func validateDrop(info: DropInfo) -> Bool {
        !store.projectFiles.busy && store.command("import_image")["enabled"].bool
            && info.hasItemsConforming(to: types)
    }
    private func fraction(_ info: DropInfo) -> Double {
        Double(min(1, max(0, info.location.y / max(1, height))))
    }
    func dropEntered(info: DropInfo) {
        if let row { feedback.update(store, row: row, fraction: fraction(info)) }
    }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        guard validateDrop(info: info) else { return DropProposal(operation: .cancel) }
        if let row { feedback.update(store, row: row, fraction: fraction(info)) }
        return DropProposal(operation: row == nil || feedback.allowed ? .copy : .cancel)
    }
    func dropExited(info: DropInfo) { feedback.clear() }
    func performDrop(info: DropInfo) -> Bool {
        defer { feedback.clear() }
        guard validateDrop(info: info) else { return false }
        let placement: JSON
        if let row { placement = JSON(["layer": ["target": row, "fraction": fraction(info)]]) }
        else { placement = JSON(["screen": ["x": Double(info.location.x * scale), "y": Double(info.location.y * scale)]]) }
        return store.projectFiles.drop(info.itemProviders(for: types), placement: placement)
    }
}
