import SwiftUI

/// UIKit menu pickers omit their label outside a Form. Keep it visible above
/// the native control, where long choices still fit narrow document sheets.
struct FormPicker<Selection: Hashable, Content: View>: View {
    let title: LocalizedStringKey
    let selection: Binding<Selection>
    let content: Content

    init(_ title: LocalizedStringKey, selection: Binding<Selection>, @ViewBuilder content: () -> Content) {
        self.title = title
        self.selection = selection
        self.content = content()
    }

    var body: some View {
        #if os(iOS)
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.subheadline).foregroundStyle(.secondary)
            picker
        }
        #else
        picker
        #endif
    }

    private var picker: some View { Picker(title, selection: selection) { content } }
}
