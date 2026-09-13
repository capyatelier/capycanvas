// Direct UIKit scrolling/contact checks. The focused XCTest workflows exercise
// actual recognition; these callbacks do not claim physical Pencil coverage.
import UIKit

@main final class RowChecks: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        Task { @MainActor in
            let model = WorkspaceRowInteraction()
            let scroll = UIScrollView(frame: CGRect(x: 0, y: 0, width: 300, height: 200))
            let input = ReorderInputView(frame: CGRect(x: 0, y: 0, width: 300, height: 24 * 56))
            input.model = model; scroll.addSubview(input); scroll.contentSize = input.bounds.size
            let ids = (0..<24).map { "Row \($0)" }
            model.items = ids.map { JSON(["id": $0]) }
            for (index, id) in ids.enumerated() {
                model.frames[id] = WorkspaceRowFrame(row: CGRect(x: 0, y: index * 56, width: 300, height: 56),
                    grip: CGRect(x: 0, y: index * 56, width: 20, height: 56))
            }
            var commits = 0
            model.commit = { _, _ in commits += 1 }
            for device in [ReorderDevice.touch, .pen, .mouse] {
                for upward in [false, true] {
                    model.cancel(); scroll.contentOffset.y = upward ? 500 : 0; input.validate()
                    let origin = CGPoint(x: 100, y: scroll.contentOffset.y + 100)
                    model.contact.prepare(model.source(at: origin)!, device: device, origin: origin)
                    if device != .mouse {
                        precondition(!model.contact.move(to: origin), "Row motion before a hold must remain scrollable")
                        model.recognizeHold(); precondition(model.menu != nil && commits == 0)
                    }
                    let edge = CGPoint(x: 100, y: scroll.contentOffset.y + (upward ? 10 : 190))
                    precondition(input.moveReorder(edge) && model.menu == nil)
                    let previous = scroll.contentOffset.y
                    for _ in 0..<12 {
                        precondition(input.trackReorder { CGPoint(x: 100, y: scroll.contentOffset.y + (upward ? 10 : 190)) })
                    }
                    precondition(upward ? scroll.contentOffset.y < previous : scroll.contentOffset.y > previous)
                    model.contact.release(at: CGPoint(x: 100, y: scroll.contentOffset.y + 100))
                    model.contact.release(at: origin)
                    precondition(commits == 1, "A completed contact commits once")
                    commits = 0
                }
                model.cancel(); scroll.contentOffset = .zero; input.validate()
                let origin = CGPoint(x: 100, y: 80)
                model.contact.prepare(model.source(at: origin)!, device: device, origin: origin)
                model.recognizeHold(); model.contact.release(at: origin)
                precondition(device == .mouse ? model.menu == nil : model.menu != nil)
                model.closeMenu()
                let grip = CGPoint(x: 10, y: 80)
                model.contact.prepare(model.source(at: grip)!, device: device, origin: grip)
                precondition(!model.contact.requiresHold && input.moveReorder(CGPoint(x: 10, y: 150)))
                model.cancel(); precondition(commits == 0 && model.drag == nil && model.menu == nil)
            }
            print("PASS: native row sessions: touch/pen/mouse pickup, hold-release menus, both edge directions, immediate grips, cancellation and one commit")
            fflush(stdout); exit(0)
        }
        return true
    }
}
