import SwiftUI
import UIKit

extension View {
    func nativeEditorContextMenu(identity: String, load: @escaping AppleContextMenuRequest,
        visibility: @escaping (Bool) -> Void) -> some View {
        NativeContextMenuHost(content: self, identity: identity, load: load, visibility: visibility)
    }
}

private struct NativeMenuContent<Content: View>: View {
    let content: Content
    let values: EnvironmentValues
    var body: some View { content.environment(\.self, values) }
}

private struct NativeContextMenuHost<Content: View>: UIViewControllerRepresentable {
    let content: Content
    let identity: String
    let load: AppleContextMenuRequest
    let visibility: (Bool) -> Void
    func makeUIViewController(context: Context) -> NativeContextMenuController<Content> {
        NativeContextMenuController(rootView: NativeMenuContent(content: content, values: context.environment))
    }
    func updateUIViewController(_ controller: NativeContextMenuController<Content>, context: Context) {
        if controller.identity != identity { controller.invalidate() }
        controller.identity = identity; controller.load = load; controller.visibility = visibility
        controller.rootView = NativeMenuContent(content: content, values: context.environment)
    }
    func sizeThatFits(_ proposal: ProposedViewSize, uiViewController: NativeContextMenuController<Content>, context: Context) -> CGSize? {
        uiViewController.sizeThatFits(in: CGSize(width: proposal.width ?? UIView.layoutFittingExpandedSize.width,
            height: proposal.height ?? UIView.layoutFittingExpandedSize.height))
    }
    static func dismantleUIViewController(_ controller: NativeContextMenuController<Content>, coordinator: ()) { controller.invalidate() }
}

private final class NativeContextMenuController<Content: View>: UIHostingController<NativeMenuContent<Content>>, UIContextMenuInteractionDelegate {
    var identity = ""
    var load: AppleContextMenuRequest = { $0(nil) }
    var visibility: (Bool) -> Void = { _ in }
    private var request = UUID()
    private var displayed = false
    private lazy var interaction = UIContextMenuInteraction(delegate: self)
    override func viewDidLoad() {
        super.viewDidLoad(); view.backgroundColor = .clear
        safeAreaRegions = []; sizingOptions = .intrinsicContentSize
        view.addInteraction(interaction)
    }
    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated); invalidate()
    }
    func invalidate() {
        request = UUID(); interaction.dismissMenu()
        if displayed { displayed = false; visibility(false) }
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, configurationForMenuAtLocation location: CGPoint) -> UIContextMenuConfiguration? {
        let ticket = UUID(); request = ticket
        return UIContextMenuConfiguration(identifier: ticket.uuidString as NSString, previewProvider: nil) { [weak self] _ in
            UIMenu(children: [UIDeferredMenuElement.uncached { completion in
                Task { @MainActor in
                    guard let self, self.request == ticket else { completion([]); return }
                    self.load { [weak self] model in
                        guard let self, self.request == ticket else { completion([]); return }
                        completion(model.map { model in
                            let native = model.nativeMenu()
                            return model.title.isEmpty ? native.children : [UIMenu(title: model.title, options: .displayInline, children: native.children)]
                        } ?? [])
                    }
                }
            }])
        }
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, willDisplayMenuFor configuration: UIContextMenuConfiguration, animator: (any UIContextMenuInteractionAnimating)?) {
        guard configuration.identifier as? String == request.uuidString else { return }
        displayed = true; visibility(true)
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, willEndFor configuration: UIContextMenuConfiguration, animator: (any UIContextMenuInteractionAnimating)?) {
        guard configuration.identifier as? String == request.uuidString else { return }
        request = UUID()
        if displayed { displayed = false; visibility(false) }
    }
}
