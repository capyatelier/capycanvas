import SwiftUI

/// Context models are queried only on activation. Never cache eligibility or
/// create a menu query for every tile during drawing/camera publication.
struct WorkspaceContext: ViewModifier {
    @ObservedObject var store: EditorStore
    let target: JSON
    var openOnTap = false
    var doubleClick: (() -> Void)?
    @State private var menu = JSON()
    @State private var generation = UUID()
    func body(content: Content) -> some View {
        content.editorContextAction(open)
            .simultaneousGesture(TapGesture(count: 2).exclusively(before: TapGesture()).onEnded { value in
                switch value { case .first: doubleClick?(); case .second: open() }
            }, isEnabled: openOnTap)
            .popover(isPresented: Binding(get: { !menu.isNull }, set: { if !$0 { menu = JSON() } })) {
                WorkspaceMenu(store: store, menu: menu) { menu = JSON() }
                    .presentationCompactAdaptation(.popover)
            }
            .onDisappear { generation = UUID(); menu = JSON() }
    }
    private func open() {
        let request = UUID(); generation = request
        store.query(["type": "context", "target": target.raw]) { result in
            guard generation == request else { return }
            menu = result
        }
    }
}

struct WorkspaceMenu: View {
    @ObservedObject var store: EditorStore
    let menu: JSON
    var width: CGFloat? = 360
    var dismiss: () -> Void = {}
    @State private var pages: [JSON] = []
    private var page: JSON { pages.last ?? menu }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                if !pages.isEmpty {
                    Button { pages.removeLast() } label: {
                        Label(page["label"].string, systemImage: "chevron.left").fontWeight(.bold)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(8)
                    }.buttonStyle(.plain).accessibilityIdentifier("workspace-menu-back")
                } else if !menu["title"].string.isEmpty {
                    Text(menu["title"].string).opacity(0.55).padding(8)
                }
                let sections = page["sections"].array.filter { !$0.array.isEmpty }
                ForEach(sections.indices, id: \.self) { section in
                    if section > 0 { Divider().padding(.vertical, 4) }
                    ForEach(sections[section].array.indices, id: \.self) { index in
                        let item = sections[section][index]
                        Button {
                            if !item["sections"].array.isEmpty { pages.append(item) }
                            else { dismiss(); store.dispatch(item["action"]) }
                        } label: {
                            HStack(spacing: 8) {
                                SharedIcon(name: "check").opacity(item["selected"].bool ? 1 : 0)
                                Text(item["label"].string).frame(maxWidth: .infinity, alignment: .leading)
                                if !item["hint"].string.isEmpty { Text(item["hint"].string).opacity(0.55) }
                                if !item["sections"].array.isEmpty { Image(systemName: "chevron.right") }
                            }.padding(.horizontal, 8).frame(minHeight: 32).contentShape(Rectangle())
                        }.buttonStyle(.plain).disabled(!item["enabled"].bool)
                            .accessibilityIdentifier("workspace-action-" + item["label"].string)
                            .accessibilityAddTraits(item["selected"].bool ? .isSelected : [])
                    }
                }
            }.padding(6)
        }.frame(width: width).frame(maxHeight: 560)
            .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace-context-menu")
    }
}

/// Local drafts keep text/caret stable while the serial owner publishes edits.
struct WorkspaceTextField: View {
    let label: String
    let value: String
    let edit: (String) -> Void
    @State private var text: String
    @State private var pending: String?
    init(_ label: String, value: String, edit: @escaping (String) -> Void) {
        self.label = label; self.value = value; self.edit = edit
        _text = State(initialValue: value)
    }
    var body: some View {
        TextField(label, text: Binding(get: { text }, set: { text = $0; pending = $0; edit($0) }))
            .textFieldStyle(.roundedBorder)
            .onChange(of: value) { _, next in
                if pending == nil || pending == next { text = next; pending = nil }
            }
    }
}
