import SwiftUI

#if os(iOS)
private struct EditorIdealHeightLayout: Layout {
    let changed: (CGSize) -> Void
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        subviews.first?.sizeThatFits(ProposedViewSize(width: proposal.width, height: nil)) ?? .zero
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard let content = subviews.first else { return }
        let ideal = content.sizeThatFits(ProposedViewSize(width: bounds.width, height: nil))
        content.place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: ideal.height))
        if ideal.width.isFinite && ideal.width > 0 && ideal.height.isFinite && ideal.height > 0 {
            DispatchQueue.main.async { changed(ideal) }
        }
    }
}

private struct EditorFittedSheet: UIViewControllerRepresentable {
    let size: CGSize
    final class Controller: UIViewController {
        var size = CGSize.zero
        private weak var fittedSheet: UISheetPresentationController?
        private weak var fittedPresenter: UIViewController?
        private var presenterTraits: (any UITraitChangeRegistration)?
        private var fittedSize = CGSize.zero
        private var fittedCompact: Bool?
        override func didMove(toParent parent: UIViewController?) {
            super.didMove(toParent: parent)
            fit()
        }
        override func viewWillAppear(_ animated: Bool) {
            super.viewWillAppear(animated)
            fit()
        }
        func stopObserving() {
            if let presenterTraits { fittedPresenter?.unregisterForTraitChanges(presenterTraits) }
            presenterTraits = nil; fittedPresenter = nil
        }
        @objc func fit() {
            guard size.width > 0 && size.height > 0 else { return }
            var ancestor = parent
            while let controller = ancestor {
                if let presenter = controller.presentingViewController,
                    presenter.presentedViewController === controller,
                    let sheet = controller.presentationController as? UISheetPresentationController {
                    if fittedPresenter !== presenter {
                        stopObserving()
                        fittedPresenter = presenter
                        presenterTraits = presenter.registerForTraitChanges([UITraitHorizontalSizeClass.self], target: self, action: #selector(fit))
                    }
                    let compact = presenter.traitCollection.horizontalSizeClass == .compact
                    let requested = compact ? CGSize(width: 0, height: size.height) : size
                    guard fittedSheet !== sheet || fittedSize != requested || fittedCompact != compact else { return }
                    fittedSheet = sheet; fittedSize = requested; fittedCompact = compact
                    if compact {
                        if !sheet.prefersPageSizing { sheet.prefersPageSizing = true }
                        sheet.detents = [.custom(identifier: .init("capy-fitted")) { [height = size.height] context in min(height, context.maximumDetentValue) }]
                    } else {
                        if sheet.prefersPageSizing { sheet.prefersPageSizing = false }
                        if controller.preferredContentSize != size { controller.preferredContentSize = size }
                    }
                    return
                }
                ancestor = controller.parent
            }
        }
    }
    func makeUIViewController(context: Context) -> Controller { Controller() }
    func updateUIViewController(_ controller: Controller, context: Context) {
        controller.size = size
        controller.fit()
    }
    static func dismantleUIViewController(_ controller: Controller, coordinator: ()) {
        controller.stopObserving()
    }
}

struct EditorLegacyFittedPresentation: ViewModifier {
    @State private var size = CGSize(width: 0, height: 500)
    func body(content: Content) -> some View {
        EditorIdealHeightLayout(changed: { if size != $0 { size = $0 } }) {
            content
        }.background(EditorFittedSheet(size: size))
    }
}
#endif
