import SwiftUI
import UIKit
import os

@MainActor private final class Samples {
    var values: [CGFloat] = []
}

private struct NestedScrolls: View {
    let samples: Samples
    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                Color.clear.frame(height: 80)
                ScrollView {
                    Color.clear.frame(height: 1600)
                        .background(NativeScrollObservation(offsetChanged: { samples.values.append($0) }))
                }.frame(height: 240)
                Color.clear.frame(height: 1000)
            }
        }
    }
}

@MainActor private final class DisplayFrame: NSObject {
    private var continuation: CheckedContinuation<Void, Never>?
    private var link: CADisplayLink?
    func wait() async {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            let link = CADisplayLink(target: self, selector: #selector(frame))
            self.link = link
            link.add(to: .main, forMode: .common)
        }
    }
    @objc private func frame() {
        link?.invalidate(); link = nil
        continuation?.resume(); continuation = nil
    }
}

@main final class ScrollChecks: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    @MainActor private func settle(_ window: UIWindow) async {
        for _ in 0..<4 {
            window.layoutIfNeeded()
            await DisplayFrame().wait()
        }
    }

    @MainActor private func observations(_ view: UIView) -> [ScrollObservationView] {
        var result = (view as? ScrollObservationView).map { [$0] } ?? []
        result += view.subviews.flatMap { observations($0) }
        return result
    }

    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        Task { @MainActor in
            print("Checking native scroll lifecycle"); fflush(stdout)
            let gate = NativeInputGate()
            for _ in 0..<8 {
                let winners = OSAllocatedUnfairLock(initialState: 0)
                DispatchQueue.concurrentPerform(iterations: 1000) { _ in
                    if gate.claim() { winners.withLock { $0 += 1 } }
                }
                precondition(winners.withLock { $0 } == 1 && !gate.claim())
                gate.release()
                precondition(gate.claim())
                gate.release()
            }
            let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 768, height: 1024))
            self.window = window
            let samples = Samples()
            let host = UIHostingController(rootView: NestedScrolls(samples: samples))
            window.rootViewController = host
            window.makeKeyAndVisible()
            await settle(window)
            let observation = observations(host.view).first!
            var ancestor = observation.superview
            while ancestor != nil && !(ancestor is UIScrollView) { ancestor = ancestor?.superview }
            let inner = ancestor as! UIScrollView
            ancestor = inner.superview
            while ancestor != nil && !(ancestor is UIScrollView) { ancestor = ancestor?.superview }
            let outer = ancestor as! UIScrollView
            let delegate = inner.delegate
            let gestureDelegate = inner.panGestureRecognizer.delegate
            let gestures = inner.gestureRecognizers!.map(ObjectIdentifier.init)
            samples.values.removeAll()
            outer.contentOffset.y = 100
            await settle(window)
            precondition(samples.values.isEmpty, "Nested observation must ignore the outer scroll view")
            inner.contentOffset.y = 31
            inner.contentOffset.y = 73
            inner.contentOffset.y = 127
            await settle(window)
            precondition(samples.values == [127 + inner.adjustedContentInset.top], "Offsets must coalesce to the exact final native value")
            precondition(inner.delegate === delegate && inner.panGestureRecognizer.delegate === gestureDelegate)
            precondition(inner.gestureRecognizers!.map(ObjectIdentifier.init) == gestures, "Observation must preserve native gesture plumbing")

            let position = EditorScrollPosition()
            observation.position = position
            position.scrollTo(y: 300)
            observation.attach()
            await settle(window)
            precondition(position.offset == 300, "An offset request must restore the recorded position")
            inner.contentOffset.y = 177 - inner.adjustedContentInset.top
            await settle(window)
            precondition(position.offset == 177, "Native scrolling must track position")
            inner.bounces = true
            inner.contentOffset.y = -inner.adjustedContentInset.top - 24
            await settle(window)
            precondition(position.offset == 0 && samples.values.last == 0, "Top overscroll must report the content start")
            position.scrollTo(y: .greatestFiniteMagnitude)
            observation.attach()
            await settle(window)
            let maximum = max(0, inner.contentSize.height - inner.bounds.height + inner.adjustedContentInset.bottom + inner.adjustedContentInset.top)
            precondition(position.offset == maximum, "Restoration must clamp to the content end")
            position.scrollTo(y: -100)
            observation.attach()
            await settle(window)
            precondition(position.offset == 0, "Restoration must clamp to the content start")

            samples.values.removeAll()
            inner.contentOffset.y = 66
            observation.detach()
            observation.removeFromSuperview()
            await settle(window)
            precondition(samples.values.isEmpty, "Removed observation must not publish queued offsets")
            let remounted = Samples()
            inner.addSubview(observation)
            observation.attach()
            inner.contentOffset.y = 88
            observation.detach()
            let replacement = EditorScrollPosition()
            replacement.scrollTo(y: 210)
            observation.position = replacement
            observation.offsetChanged = { remounted.values.append($0) }
            inner.addSubview(observation)
            observation.attach()
            await settle(window)
            precondition(replacement.offset == 210, "Remount must apply a pending restoration")
            precondition(samples.values.isEmpty && remounted.values == [210], "Queued events must not publish to a retired owner or duplicate the remount publication")
            let selection = EditorScrollPosition()
            var publishedOffset: CGFloat?
            @MainActor func positioned() -> UIViewController {
                UIHostingController(rootView: EditorPositionedScrollView(position: selection, offsetChanged: { publishedOffset = $0 }) {
                    VStack(spacing: 0) {
                        ForEach(0..<40, id: \.self) { index in
                            Color.clear.frame(height: 60).id(UInt64(index))
                        }
                    }
                })
            }
            @MainActor func scrollView(_ view: UIView) -> UIScrollView? {
                if let scroll = view as? UIScrollView { return scroll }
                for child in view.subviews { if let scroll = scrollView(child) { return scroll } }
                return nil
            }
            window.rootViewController = positioned()
            await settle(window)
            selection.scrollTo(id: 35)
            await settle(window)
            precondition(selection.offset > 1000, "Selecting an item must reveal a distant row")
            selection.scrollTo(y: 120)
            await settle(window)
            print("Positioned offset after restore: \(selection.offset)"); fflush(stdout)
            precondition(abs(selection.offset - 120) < 1, "Positioned scroll must restore an offset after item navigation")
            window.rootViewController = UIViewController()
            await settle(window)
            window.rootViewController = positioned()
            await settle(window)
            precondition(abs(selection.offset - 120) < 1, "A fresh positioned scroll must preserve the owner's offset across remount")

            let mountedScroll = scrollView(window.rootViewController!.view)!
            mountedScroll.setContentOffset(CGPoint(x: 0, y: 180 - mountedScroll.adjustedContentInset.top), animated: false)
            await settle(window)
            precondition(abs(selection.offset - 180) < 1, "Native movement must replace the previously requested offset")
            mountedScroll.bounces = true
            mountedScroll.setContentOffset(CGPoint(x: 0, y: -mountedScroll.adjustedContentInset.top - 24), animated: false)
            await settle(window)
            precondition(selection.offset == 0 && publishedOffset == 0, "Positioned scroll must report the content start during top overscroll")
            mountedScroll.setContentOffset(CGPoint(x: 0, y: 180 - mountedScroll.adjustedContentInset.top), animated: false)
            await settle(window)
            window.rootViewController = UIViewController()
            await settle(window)
            window.rootViewController = positioned()
            await settle(window)
            precondition(abs(selection.offset - 180) < 1, "Remount must preserve native movement instead of replaying an applied request")

            window.rootViewController = UIViewController()
            await settle(window)
            selection.scrollTo(y: 240)
            window.rootViewController = positioned()
            await settle(window)
            precondition(abs(selection.offset - 240) < 1, "Remount must apply an offset requested while the scroll was absent")
            window.rootViewController = UIViewController()
            await settle(window)
            selection.scrollTo(id: 35)
            window.rootViewController = positioned()
            await settle(window)
            precondition(selection.offset > 1000, "Remount must reveal an item requested while the scroll was absent")
            print("PASS: native scroll observation: nearest scroll, exact coalescing, tracking, clamped restoration, removal and remount, native delegates and gestures")
            fflush(stdout)
            exit(0)
        }
        return true
    }
}
