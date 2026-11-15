import AppKit
import SwiftUI

@main struct DialogPageLayoutChecks {
    @MainActor static func main() {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await check(); exit(0) }
            catch { FileHandle.standardError.write(Data("FAIL: \(error)\n".utf8)); exit(1) }
        }
        NSApp.run()
    }
    @MainActor static func fields(_ view: NSView) -> [NSTextField] {
        (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
    }
    @MainActor static func check() async throws {
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 402, height: 874),
            styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ description: String, until ready: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(15)
            while !ready() {
                if let failure = store.failure { throw HostFailure(message: failure) }
                guard Date() < deadline else { throw HostFailure(message: description) }
                window.contentView?.layoutSubtreeIfNeeded()
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        try await wait("Dialog catalog loads") { !store.catalog.isNull }
        let profile = NSHostingView(rootView: PhotoProfileForm(preferences: store.colorPreferences,
            interpretation: JSON(["channels": "Rgb", "depth": "U8"]), error: nil, busy: false, choose: { _ in }).environment(\.editorDialogPage, true)
            .environment(\.capyProfileCopy, store.catalog["profile_copy"]))
        window.setContentSize(CGSize(width: 320, height: 874)); window.contentView = profile
        var profilePicker: NSPopUpButton?
        try await wait("The actual photo profile picker mounts") {
            var pending = [profile as NSView]
            while let view = pending.popLast() {
                if let picker = view as? NSPopUpButton { profilePicker = picker; return true }
                pending += view.subviews
            }
            return false
        }
        profile.layoutSubtreeIfNeeded()
        var ancestors: NSView? = profilePicker
        while let view = ancestors, !(view is NSScrollView) { ancestors = view.superview }
        guard ancestors is NSScrollView else { throw HostFailure(message: "The photo profile page body must have a native scroll container") }
        let pickerFrame = profile.convert(profilePicker!.bounds, from: profilePicker!)
        let pickerTop = profile.isFlipped ? pickerFrame.minY : profile.bounds.height - pickerFrame.maxY
        guard pickerTop < profile.bounds.height / 3 else {
            throw HostFailure(message: "Photo profile page picker must remain near the top: picker=\(pickerFrame), bounds=\(profile.bounds), top=\(pickerTop)")
        }
        print("PASS: Photo profile page places its native picker below the top header")
        for width: CGFloat in [320, 402] {
            try await store.apply(["type": "invoke", "command": "new_document"])
            try await wait("New drawing opens") { !store.state["new_drawing"].isNull }
            let host = NSHostingView(rootView: NewDrawingDialog(store: store).environment(\.editorDialogPage, true)
                .environment(\.editorPopupStore, store).environment(\.capyCommonCopy, store.bootstrap["common"]))
            window.setContentSize(CGSize(width: width, height: 874)); window.contentView = host
            try await wait("New drawing lays out its native name field") { fields(host).contains { $0.isEditable } }
            host.layoutSubtreeIfNeeded()
            for field in fields(host).filter(\.isEditable) {
                let frame = host.convert(field.bounds, from: field)
                guard frame.minX >= -0.5 && frame.maxX <= width + 0.5 else {
                    throw HostFailure(message: "New drawing field exceeds its \(width) point allocation: \(frame)")
                }
            }
            print("PASS: New drawing uses the \(Int(width)) point page allocation")
            try await store.projectFiles.newDrawingStep(["op": "cancel"])
            try await wait("New drawing closes before its native host retires") { store.state["new_drawing"].isNull }
            host.layoutSubtreeIfNeeded()
            let fitted = host.fittingSize
            guard fitted.width.isFinite && fitted.height.isFinite else {
                throw HostFailure(message: "Cleared page header must keep finite native layout")
            }
            print("PASS: Cleared New drawing header lays out at \(Int(width)) points")
        }
        try await store.apply(["type": "customize", "action": ["type": "rename_toolbar", "panel": "toolbar"]])
        precondition(store.snapshot["toolbar_prompt"]["dialog"]["role"].string == "editor")
        let content = WorkspaceDialog(store: store).environment(\.editorPopupStore, store).font(.system(size: 44 / 3))
        let rename = NSHostingView(rootView: content.environment(\.editorDialogPage, true))
        window.contentView = rename
        try await wait("The actual Rename page mounts its name field") { fields(rename).contains { $0.isEditable } }
        let name = fields(rename).first { $0.isEditable }!
        rename.rootView = content.environment(\.editorDialogPage, false)
        rename.layoutSubtreeIfNeeded()
        precondition(fields(rename).first { $0.isEditable } === name,
            "Rename must keep its native field across page and floating layout")
        try await store.apply(["type": "customize", "action": ["type": "cancel_toolbar"]])
        try await store.apply(["type": "customize", "action": ["type": "delete_toolbar", "panel": "toolbar"]])
        precondition(store.snapshot["toolbar_prompt"]["dialog"]["role"].string == "alert")
        store.native?.resize(width: 320, height: 568, scale: 1)
        try await wait("The toolbar alert viewport publishes") { store.snapshot["layout"]["viewport"][0].number == 320 }
        let prompt = NSHostingView(rootView: content.environment(\.editorDialogPage, false))
        window.setContentSize(CGSize(width: 320, height: 568)); window.contentView = prompt
        prompt.layoutSubtreeIfNeeded()
        let fitted = prompt.fittingSize
        var pending = [prompt as NSView], scrolls: [NSScrollView] = []
        while let view = pending.popLast() {
            if let scroll = view as? NSScrollView { scrolls.append(scroll) }
            pending += view.subviews
        }
        precondition(abs(fitted.width - 300) <= 1,
            "The actual Delete Toolbar body retains the shared alert minimum in a 320 point allocation")
        precondition(fitted.height < 568, "A short confirmation must fit below its available height")
        precondition(!store.snapshot["toolbar_prompt"]["message"].string.isEmpty && scrolls.count == 1)
        precondition(scrolls[0].documentView!.bounds.height > 0
            && scrolls[0].documentView!.bounds.height <= scrolls[0].contentView.bounds.height + 1,
            "The rendered Delete message must fit in the compact alert body")
        try await store.apply(["type": "customize", "action": ["type": "cancel_toolbar"]])
        try await wait("The toolbar alert closes") { store.snapshot["toolbar_prompt"].isNull }
        print("PASS: Toolbar prompt roles, field lifetime and actual compact shared alert allocation")
        try await store.apply(["type": "customize", "action": ["type": "insert_tools", "panel": "commands", "before": NSNull()]])
        let tools = NSHostingView(rootView: content.environment(\.editorDialogPage, false))
        window.styleMask = [.titled, .closable]; window.contentView = tools
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await wait("The floating Insert Tools picker mounts") { fields(tools).contains { $0.isEditable } }
        try require(window.makeFirstResponder(nil), "The floating picker must have no focused field")
        try NativeWorkspaceInputFixture.key("\u{1b}", code: 53, window: window)
        try await wait("Escape without a focused field closes Insert Tools") { store.snapshot["picker"].isNull }
        print("PASS: Floating Insert Tools retains native Escape without focused text")
    }
}
