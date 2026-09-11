import UIKit

/// Shared Rust authorizes closing; UIKit owns destruction of this scene only.
@MainActor enum DocumentScene {
    static func close(_ scene: UIWindowScene?, store: EditorStore) {
        guard let scene else { failed("The drawing window is unavailable", store: store); return }
        UIApplication.shared.requestSceneSessionDestruction(scene.session, options: nil) { [weak store] error in
            DispatchQueue.main.async {
                if let store { failed(error.localizedDescription, store: store) }
            }
        }
    }
    private static func failed(_ message: String, store: EditorStore) {
        store.native?.documentRequest(closeDecision: 4) { _ in }
        store.projectFiles.error = message
    }
}
