import SwiftUI

@main struct CapyCanvasMacApp: App {
    @StateObject private var store = EditorStore(platform: 1)
    var body: some Scene {
        WindowGroup {
            EditorView(store: store) { MacMetalCanvas(store: store) }.frame(minWidth: 700, minHeight: 500)
        }.windowStyle(.hiddenTitleBar)
    }
}
