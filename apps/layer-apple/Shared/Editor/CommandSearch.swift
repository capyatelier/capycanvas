import SwiftUI

struct CommandSearchLayer: View {
    let store: EditorStore
    var body: some View {
        let view = store.state["command_search"]
        if !view.isNull && !store.catalog.isNull {
            CommandSearchBar(store: store, view: view)
        }
    }
}

private struct CommandSearchBar: View {
    let store: EditorStore
    let view: JSON
    @State private var text = ""
    @State private var selection: TextSelection?
    @State private var shown = false
    @State private var origin = "canvas"
    #if os(iOS)
    @State private var focused = false
    #else
    @FocusState private var focused: Bool
    #endif
    @Environment(\.editorPalette) private var palette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private var style: JSON { store.catalog["command_search_style"] }
    private var inset: CGFloat { style["inset"].isNull ? 12 : style["inset"].number }
    private var gap: CGFloat { style["gap"].isNull ? 8 : style["gap"].number }
    private var textSize: CGFloat { store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3 }
    private var parameter: JSON { view["parameter"] }
    private var results: [JSON] { view["results"].array }
    private var selected: Int { Int(view["selected"].uint) }
    private func rowHeight(narrow: Bool) -> CGFloat {
        let shared = style["row_height"].isNull ? 44 : style["row_height"].number
        #if os(iOS)
        return max(shared, 48)
        #else
        return narrow ? max(shared, 48) : shared
        #endif
    }
    private func send(_ action: [String: Any]) {
        store.dispatch(["type": "command_search", "action": action])
    }
    private func back() { send(["type": parameter.isNull ? "close" : "back"]) }
    private var detail: String { view["detail"].string }
    private var radius: CGFloat { style["radius"].isNull ? 12 : style["radius"].number }
    private func top(_ height: CGFloat) -> CGFloat {
        let low = style["top_min"].isNull ? 48 : style["top_min"].number, high = style["top_max"].isNull ? 192 : style["top_max"].number
        return min(max(height / 5, low), high)
    }
    private func explanation(_ command: JSON) -> String {
        command["disabled_reason"].isNull ? command["description"].string : command["disabled_reason"].string
    }

    var body: some View {
        GeometryReader { geometry in
            let narrow = geometry.size.width <= 600
            let top = narrow ? 16 : top(geometry.size.height)
            let width = style["width"].isNull ? 560 : style["width"].number
            ZStack(alignment: .top) {
                Color.clear.contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).onEnded { _ in send(["type": "close"]) })
                    .accessibilityHidden(true)
                card(rowHeight: rowHeight(narrow: narrow))
                    .frame(width: max(0, min(width, geometry.size.width - 32)))
                    .frame(maxHeight: max(0, geometry.size.height - top - 16), alignment: .top)
                    .padding(.top, top)
                    .opacity(shown ? 1 : 0).offset(y: shown || reduceMotion ? 0 : -4)
            }
        }
        .onAppear {
            origin = store.commandFocus()
            text = view["query"].string
            focused = true
            withAnimation(reduceMotion ? nil : .timingCurve(0.2, 0.8, 0.2, 1, duration: 0.12)) { shown = true }
        }
        .onDisappear { if origin != "text" { store.focusCanvas?() } }
        .onChange(of: parameter["id"].string) { _, _ in
            text = parameter.isNull ? view["query"].string : parameter["parameter"]["text"].string
            selection = parameter.isNull ? nil : TextSelection(range: text.startIndex..<text.endIndex)
        }
    }

    private func card(rowHeight: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: gap) {
            HStack(spacing: gap) {
                HStack(spacing: 6) {
                    SharedIcon(name: "search").opacity(0.65)
                    field
                        .onChange(of: text) { _, value in
                            if parameter.isNull && value != view["query"].string { send(["type": "query", "text": value]) }
                        }
                    let unit = parameter["parameter"]["numeric"]["unit"].string
                    if !unit.isEmpty { Text(unit).opacity(0.65) }
                }
                .padding(.horizontal, 8).frame(minHeight: rowHeight - 12)
                .background(palette["input"], in: RoundedRectangle(cornerRadius: 7))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(focused ? palette.accent.opacity(0.7) : palette["text"].opacity(0.18),
                    lineWidth: focused ? 2 : 1))
                Button { send(["type": "close"]) } label: {
                    SharedIcon(name: "close").frame(width: rowHeight - 12, height: rowHeight - 12).contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityIdentifier("command-search-close").accessibilityLabel("Close command search")
            }
            if parameter.isNull {
                if results.isEmpty {
                    Text("No matching commands").opacity(0.65).frame(maxWidth: .infinity).padding(inset)
                } else {
                    ScrollViewReader { proxy in
                        ViewThatFits(in: .vertical) {
                            rows(rowHeight: rowHeight)
                            ScrollView { rows(rowHeight: rowHeight) }
                        }
                        .onChange(of: selected) { _, index in proxy.scrollTo(index) }
                    }
                }
            }
            Text(detail).font(.system(size: textSize * 0.85)).opacity(0.65).lineLimit(1).truncationMode(.tail)
                .padding(.horizontal, inset).frame(minHeight: 20, alignment: .leading)
                .help(detail).accessibilityIdentifier("command-detail")
        }
        .padding(inset)
        .foregroundStyle(palette["text"])
        .glassSurface(SquircleShape(radius), fill: palette.glassPanel)
        .overlay(SquircleShape(radius).stroke(palette["text"].opacity(0.1)))
        .shadow(color: .black.opacity(0.27), radius: 12, y: 8)
        .accessibilityElement(children: .contain).accessibilityIdentifier("command-bar")
    }

    @ViewBuilder private var field: some View {
        let label = parameter.isNull ? "Search commands" : parameter["label"].string
        let prompt = parameter.isNull ? "Search commands" : "Enter a value"
        #if os(iOS)
        NumericTextField(label: label, text: $text, focused: $focused, fontSize: textSize, color: palette["text"],
            identifier: "command-search", submit: { toCanvas in
                if toCanvas { send(["type": "commit", "text": text]) }
                return true
            }, cancel: back, step: { _ = move(-$0) }, placeholder: prompt, alignment: .natural, returnKey: .search,
            selectsReplacedText: true)
            .frame(height: 24)
        #else
        TextField(prompt, text: $text, selection: $selection)
            .textFieldStyle(.plain).editorSearchInput().focused($focused)
            .accessibilityIdentifier("command-search").accessibilityLabel(label)
            .onSubmit { send(["type": "commit", "text": text]); focused = true }
            .onKeyPress(.escape) { back(); return .handled }
            .onKeyPress(.upArrow) { move(-1) }
            .onKeyPress(.downArrow) { move(1) }
        #endif
    }

    private func move(_ delta: Int) -> KeyPress.Result {
        guard parameter.isNull else { return .ignored }
        send(["type": "move", "delta": delta])
        return .handled
    }

    private func rows(rowHeight: CGFloat) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(results.enumerated()), id: \.element["id"].string) { index, command in
                row(command, index: index, height: rowHeight).id(index)
            }
        }
    }

    private func row(_ command: JSON, index: Int, height: CGFloat) -> some View {
        let enabled = command["enabled"].bool, current = index == selected
        return Button { send(["type": "execute", "id": command["id"].string, "value": NSNull()]); focused = true } label: {
            HStack(spacing: inset) {
                Text(command["label"].string).lineLimit(1).truncationMode(.tail)
                    .opacity(enabled ? 1 : 0.5).frame(maxWidth: .infinity, alignment: .leading)
                if command["selected"].bool { SharedIcon(name: "check").accessibilityLabel("On") }
                Text(command["shortcut"].string).font(.system(size: textSize * 0.85)).opacity(0.65).lineLimit(1)
            }
            .padding(.horizontal, inset).frame(minHeight: height)
            .background(current ? palette["text"].opacity(0.1) : .clear, in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onContinuousHover { phase in
            if case .active = phase, !current { send(["type": "select", "id": command["id"].string]) }
        }
        .accessibilityIdentifier("command-result-\(index)")
        .accessibilityHint(explanation(command))
        .accessibilityAddTraits(current ? .isSelected : [])
        .accessibilityValue(enabled ? "" : "Unavailable")
    }
}
