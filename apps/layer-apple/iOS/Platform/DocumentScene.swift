import UIKit

/// Shared Rust authorizes closing; UIKit owns destruction of this scene only.
@MainActor enum DocumentScene {
    static func close(_ scene: UIWindowScene?, store: EditorStore) {
        guard let scene else { failed("The drawing window is unavailable", store: store); return }
        store.prepareClose { saved in
            guard saved else { failed("Some changes could not be saved. Retry before closing this window.", store: store); return }
            UIApplication.shared.requestSceneSessionDestruction(scene.session, options: nil) { [weak store] error in
                DispatchQueue.main.async {
                    if let store { failed(error.localizedDescription, store: store) }
                }
            }
        }
    }
    private static func failed(_ message: String, store: EditorStore) {
        store.cancelPreparedClose()
        store.projectFiles.error = message
    }
}
