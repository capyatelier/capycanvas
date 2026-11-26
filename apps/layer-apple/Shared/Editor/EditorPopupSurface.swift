import SwiftUI

#if os(iOS)
struct PhoneSystemOverlays: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        if UIDevice.current.userInterfaceIdiom == .phone {
            content.statusBarHidden().persistentSystemOverlays(.hidden)
        } else { content }
    }
}

@available(iOS 18.0, *)
private struct EditorPreferredPresentationSizing: PresentationSizing {
    func proposedSize(for root: PresentationSizingRoot, context: PresentationSizingContext) -> ProposedViewSize {
        ProposedViewSize(root.sizeThatFits(.unspecified))
    }
}
#endif

private struct EditorDialogAvailableHeightKey: EnvironmentKey { static let defaultValue: CGFloat? = nil }
private struct EditorDialogPageKey: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var editorDialogAvailableHeight: CGFloat? {
        get { self[EditorDialogAvailableHeightKey.self] }
        set { self[EditorDialogAvailableHeightKey.self] = newValue }
    }
    var editorDialogPage: Bool {
        get { self[EditorDialogPageKey.self] }
        set { self[EditorDialogPageKey.self] = newValue }
    }
}

struct EditorFittedPresentation: ViewModifier {
    @Environment(\.editorDialogPage) private var page
    @Environment(\.editorDialogAvailableHeight) private var availableHeight
    @ViewBuilder func body(content: Content) -> some View {
        #if os(iOS)
        if page {
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if availableHeight != nil { content }
        else if #available(iOS 18.0, *) {
            content.fixedSize(horizontal: false, vertical: true)
                .presentationSizing(EditorPreferredPresentationSizing())
        } else {
            content.modifier(EditorLegacyFittedPresentation())
        }
        #else
        content.presentationSizing(.fitted)
        #endif
    }
}

struct EditorDialogPresentation: ViewModifier {
    let identifier: String
    #if os(macOS)
    var idealHeight: CGFloat = 420
    #else
    var idealHeight: CGFloat = 500
    #endif
    private var widths: (ideal: CGFloat, max: CGFloat) {
        #if os(macOS)
        return (720, 720)
        #else
        return (800, 860)
        #endif
    }
    func body(content: Content) -> some View {
        content.modifier(EditorDialogFrame(minWidth: 380, idealWidth: widths.ideal, maxWidth: widths.max, minHeight: 420, idealHeight: idealHeight, maxHeight: 620))
            .accessibilityElement(children: .contain).accessibilityIdentifier(identifier)
            .modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
    }
}

struct EditorDialogTitle: View {
    let text: String
    var body: some View { Text(text).font(.headline).frame(maxWidth: .infinity) }
}

struct EditorDialogFooter<Leading: View>: View {
    @Environment(\.capyCommonCopy) private var commonCopy
    @Environment(\.editorPopupStore) private var store
    let label: String
    let identifiers: (cancel: String, apply: String)
    var cancelsOnEscape = true
    var blocked = false
    var showsActions = true
    let cancel: () -> Void
    let apply: () -> Void
    @ViewBuilder let leading: () -> Leading
    private var palette: EditorPalette { EditorPalette(source: store?.state["palette"] ?? JSON()) }
    var body: some View {
        VStack(spacing: 0) {
            EditorLine(color: palette.settingsBorder)
            HStack(spacing: 6) {
                leading()
                if showsActions {
                    Button(commonCopy["cancel"].string, action: cancel).buttonStyle(.bordered)
                        .keyboardShortcut(cancelsOnEscape ? .cancelAction : nil).accessibilityIdentifier(identifiers.cancel)
                    Button(label, action: apply)
                        .buttonStyle(.borderedProminent).tint(palette.accent)
                        .disabled(blocked).keyboardShortcut(.defaultAction).accessibilityIdentifier(identifiers.apply)
                }
            }.padding(.horizontal, 20)
            #if os(macOS)
            .padding(.top, 16).padding(.bottom, 20)
            #else
            .padding(.vertical, 12)
            #endif
        }
    }
}

/// Keep text on an opaque surface from the same shared palette. Drawing content
/// must not change the contrast of a popup while it is open.
struct EditorPopupSurface<S: Shape>: ViewModifier {
    let shape: S
    @Environment(\.editorPopupStore) private var store
    private var source: JSON { store?.state["palette"] ?? JSON() }

    @ViewBuilder func body(content: Content) -> some View {
        if !source["panel"].isNull && !source["text"].isNull {
            let palette = EditorPalette(source: source)
            content.foregroundStyle(palette["text"]).background(palette["panel"], in: shape).environment(\.onSettingsSurface, false)
        } else {
            content.foregroundStyle(.primary).background(.background, in: shape).environment(\.onSettingsSurface, false)
        }
    }
}

private struct EditorPopupStoreKey: EnvironmentKey {
    static let defaultValue: EditorStore? = nil
}
extension EnvironmentValues {
    var editorPopupStore: EditorStore? {
        get { self[EditorPopupStoreKey.self] }
        set { self[EditorPopupStoreKey.self] = newValue }
    }
}

/// Set both sides of the color pair, including the popover arrow and sheet
/// margins that a content-only background would leave translucent.
struct EditorPopupPresentation: ViewModifier {
    @Environment(\.editorPopupStore) private var store
    @State private var popup = UUID()
    private var source: JSON { store?.state["palette"] ?? JSON() }
    func body(content: Content) -> some View {
        Group {
            if !source["settings"].isNull && !source["text"].isNull {
                let palette = EditorPalette(source: source)
                content.modifier(EditorPopoverHost()).foregroundStyle(palette["text"]).presentationBackground(palette["settings"])
                    .environment(\.onSettingsSurface, true).modifier(EditorPresentationAppearance())
            } else {
                content.modifier(EditorPopoverHost()).foregroundStyle(.primary).presentationBackground(.background)
            }
        }
        #if os(macOS)
        .buttonStyle(.bordered)
        #endif
        .onAppear { store?.workspace.popover(popup, open: true, defersLanguage: false) }
        .onDisappear { store?.workspace.popover(popup, open: false) }
    }
}

/// A presented sheet keeps its own appearance; read the live resolved theme
/// instead of retaining the environment value captured when it opened.
struct EditorPresentationAppearance: ViewModifier {
    @Environment(\.editorPopupStore) private var store
    func body(content: Content) -> some View {
        content.preferredColorScheme(store.map { $0.state["theme"].string == "dark" ? .dark : .light })
    }
}

struct HostSurfacePresence: ViewModifier {
    let store: EditorStore
    let shown: Bool
    @State private var id = UUID()
    func body(content: Content) -> some View {
        content.onAppear { store.hostSurface(id, shown: shown) }
            .onChange(of: shown) { _, shown in store.hostSurface(id, shown: shown) }
            .onDisappear { store.hostSurface(id, shown: false) }
    }
}

extension View {
    func editorSheet<Sheet: View>(intent: JSON, isPresented: Binding<Bool>, onDismiss: (() -> Void)? = nil,
        @ViewBuilder content: @escaping () -> Sheet) -> some View {
        modifier(EditorEnvironmentSheet(intent: intent, presented: isPresented, onDismiss: onDismiss, sheet: content))
    }
    func editorSheet<Item: Identifiable, Sheet: View>(intent: JSON, item: Binding<Item?>, onDismiss: (() -> Void)? = nil,
        @ViewBuilder content: @escaping (Item) -> Sheet) -> some View {
        editorSheet(intent: intent, isPresented: Binding(get: { item.wrappedValue != nil }, set: { if !$0 { item.wrappedValue = nil } }),
            onDismiss: onDismiss) { if let value = item.wrappedValue { content(value).id(value.id) } }
    }

    func editorSheet<Sheet: View>(store: EditorStore, intent: JSON, isPresented: Binding<Bool>,
        onDismiss: (() -> Void)? = nil, @ViewBuilder content: @escaping () -> Sheet) -> some View {
        modifier(EditorSheet(store: store, intent: intent, presented: isPresented, onDismiss: onDismiss, sheet: content))
    }
}

private struct EditorEnvironmentSheet<Sheet: View>: ViewModifier {
    @Environment(\.editorPopupStore) private var store
    let intent: JSON
    @Binding var presented: Bool
    var onDismiss: (() -> Void)?
    @ViewBuilder let sheet: () -> Sheet
    @ViewBuilder func body(content: Content) -> some View {
        if let store { content.editorSheet(store: store, intent: intent, isPresented: $presented, onDismiss: onDismiss, content: sheet) }
        else { content.sheet(isPresented: $presented, onDismiss: onDismiss, content: sheet) }
    }
}

private struct EditorSheet<Sheet: View>: ViewModifier {
    @ObservedObject var store: EditorStore
    let intent: JSON
    @Binding var presented: Bool
    var onDismiss: (() -> Void)?
    @ViewBuilder let sheet: () -> Sheet
    @State private var closing = false
    private var presentation: JSON {
        let layout = store.snapshot["layout"], insets = store.state["phone"]["insets"]
        guard !intent["role"].isNull, layout["viewport"].array.count == 2, insets.array.count == 4 else { return JSON() }
        return DialogUI.resolve(["kind": "presentation", "role": intent["role"].raw, "viewport": layout["viewport"].raw, "insets": insets.raw,
            "safe_area": store.state["phone"]["safe_area"].raw, "work_area": layout["phone"]["work_area"].raw, "visible": visible])
    }
    private var alert: Bool { !store.snapshot["layout"]["phone"].isNull && intent["role"].string == "alert" }
    private var visible: [String: Any] {
        let viewport = store.snapshot["layout"]["viewport"]
        return ["x": 0, "y": 0, "width": viewport[0].number,
            "height": max(0, viewport[1].number - Double(store.obscuredBottom))]
    }
    private func binding(page: Bool) -> Binding<Bool> {
        Binding(get: { presented && !alert && !closing && !presentation.isNull && (presentation["kind"].string == "page") == page },
            set: { value in
                if !value && presented && !alert && !presentation.isNull && (presentation["kind"].string == "page") == page {
                    closing = true; presented = false
                }
            })
    }
    private func dismissed() { if closing || !presented { onDismiss?() } }
    @ViewBuilder func body(content: Content) -> some View {
        #if os(iOS)
        content.sheet(isPresented: binding(page: false), onDismiss: dismissed) {
            sheet().environment(\.editorDialogPage, false).environment(\.editorDialogAvailableHeight, nil)
        }.fullScreenCover(isPresented: binding(page: true), onDismiss: dismissed) {
            GeometryReader { _ in
                ZStack(alignment: .topLeading) {
                    EditorPalette(source: store.state["palette"])["settings"].ignoresSafeArea()
                    sheet().environment(\.editorDialogPage, true)
                        .environment(\.editorDialogAvailableHeight, CGFloat(presentation["bounds"]["height"].number))
                        .placed(presentation["bounds"])
                }
            }.ignoresSafeArea(.container)
                .modifier(PhoneSystemOverlays())
        }.modifier(EditorAlertSurface(store: store, presented: presented && alert, onDismiss: dismissed) { size in
            sheet().environment(\.editorDialogPage, false).environment(\.editorDialogAvailableHeight, size.height)
        }).onChange(of: presented) { _, shown in if !shown { closing = false } }
            .modifier(HostSurfacePresence(store: store, shown: presented))
        #else
        content.sheet(isPresented: $presented, onDismiss: onDismiss, content: sheet)
            .modifier(HostSurfacePresence(store: store, shown: presented))
        #endif
    }
}

struct EditorPageHeader: View {
    let intent: JSON
    let title: String
    var identifier = "dialog"
    var subpage = false
    var primaryLabel = ""
    var primaryEnabled = true
    var primaryIdentifier: String? = nil
    var backIdentifier: String? = nil
    var cancelIdentifier: String? = nil
    var cancelsOnEscape = true
    let cancel: () -> Void
    var back: (() -> Void)? = nil
    var primary: (() -> Void)? = nil
    @Environment(\.capyCommonCopy) private var common
    @Environment(\.editorPalette) private var palette
    private var header: JSON { DialogUI.resolve(["kind": "header", "intent": intent.raw, "subpage": subpage]) }
    var body: some View {
        EditorPageHeaderLayout {
            ZStack {
                if !header["start"].isNull {
                    Button(action: { if header["start"].string == "back" { back?() } else { cancel() } }) {
                        SharedIcon(name: header["start"].string == "back" ? "back" : "close")
                    }.accessibilityLabel(common[header["start"].string == "back" ? "back" : "cancel"].string)
                        .accessibilityIdentifier(header["start"].string == "back" ? backIdentifier ?? identifier + "-back" : cancelIdentifier ?? identifier + "-cancel").keyboardShortcut(cancelsOnEscape ? .cancelAction : nil)
                }
            }.frame(width: 56)
            Text(title).font(.headline).accessibilityIdentifier(identifier + "-title")
            ZStack {
                if header["end"].string == "primary" {
                    Button(primaryLabel) { primary?() }.disabled(!primaryEnabled).buttonStyle(.borderedProminent)
                        .foregroundStyle(palette.accentForeground)
                        .accessibilityIdentifier(primaryIdentifier ?? identifier + "-apply").keyboardShortcut(.defaultAction)
                } else if header["end"].string == "close" {
                    Button(common["done"].string, action: cancel).buttonStyle(.borderedProminent)
                        .foregroundStyle(palette.accentForeground)
                        .accessibilityIdentifier(cancelIdentifier ?? identifier + "-done").keyboardShortcut(.cancelAction)
                }
            }.frame(minWidth: 56)
        }.padding(.leading, 8).padding(.trailing, 20).frame(minHeight: 48)
    }
}

private struct EditorPageHeaderLayout: Layout {
    private func sizes(_ subviews: Subviews, width: CGFloat) -> [CGSize] {
        let start = subviews[0].sizeThatFits(ProposedViewSize(width: 56, height: nil))
        let end = subviews[2].sizeThatFits(ProposedViewSize(width: max(0, width - start.width - 48), height: nil))
        let title = subviews[1].sizeThatFits(ProposedViewSize(width: max(0, width - start.width - end.width - 12), height: nil))
        return [start, title, end]
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? subviews.map { $0.sizeThatFits(.unspecified).width }.reduce(0, +) + 12
        return CGSize(width: width, height: max(48, sizes(subviews, width: width).map(\.height).max() ?? 0))
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let sizes = sizes(subviews, width: bounds.width)
        let lower = sizes[0].width + 6
        let upper = max(lower, bounds.width - sizes[2].width - sizes[1].width - 6)
        let title = min(upper, max(lower, (bounds.width - sizes[1].width) / 2))
        for (index, x) in [CGFloat.zero, title, bounds.width - sizes[2].width].enumerated() {
            subviews[index].place(at: CGPoint(x: bounds.minX + x, y: bounds.midY - sizes[index].height / 2), proposal: ProposedViewSize(sizes[index]))
        }
    }
}


struct EditorDialogFrame: ViewModifier {
    var minWidth: CGFloat? = nil
    var idealWidth: CGFloat? = nil
    var maxWidth: CGFloat? = nil
    var minHeight: CGFloat? = nil
    var idealHeight: CGFloat? = nil
    var maxHeight: CGFloat? = nil
    @Environment(\.editorDialogPage) private var page
    @Environment(\.editorDialogAvailableHeight) private var availableHeight
    func body(content: Content) -> some View {
        content.frame(minWidth: page ? 0 : minWidth, idealWidth: page ? nil : idealWidth, maxWidth: page ? .infinity : maxWidth,
            minHeight: page ? 0 : minHeight.map { min($0, availableHeight ?? .infinity) }, idealHeight: page ? nil : idealHeight, maxHeight: page ? .infinity : maxHeight, alignment: page ? .topLeading : .center)
    }
}

struct EditorAlertResponse: Identifiable {
    var id: String { identifier }
    let label: String
    let identifier: String
    var accent = false
    var destructive = false
    var cancel = false
    var enabled = true
    let action: () -> Void
}

extension View {
    func editorAlert(intent: JSON, title: String, message: String = "", isPresented: Binding<Bool>, responses: [EditorAlertResponse], bodyContent: AnyView? = nil) -> some View {
        modifier(EditorEnvironmentAlert(intent: intent, title: title, message: message, presented: isPresented, responses: responses, bodyContent: bodyContent))
    }
    func editorAlert(store: EditorStore, intent: JSON, title: String, message: String = "", isPresented: Binding<Bool>,
        responses: [EditorAlertResponse], bodyContent: AnyView? = nil) -> some View {
        modifier(EditorAlertPresentation(store: store, intent: intent, title: title, message: message, presented: isPresented, responses: responses, bodyContent: bodyContent))
    }
}

private struct EditorEnvironmentAlert: ViewModifier {
    @Environment(\.editorPopupStore) private var store
    let intent: JSON
    let title: String
    let message: String
    @Binding var presented: Bool
    let responses: [EditorAlertResponse]
    var bodyContent: AnyView? = nil
    @ViewBuilder func body(content: Content) -> some View {
        if let store { content.editorAlert(store: store, intent: intent, title: title, message: message, isPresented: $presented, responses: responses, bodyContent: bodyContent) }
        else {
            content.alert(title, isPresented: $presented) {
                ForEach(responses) { response in
                    Button(response.label, role: response.destructive ? .destructive : response.cancel ? .cancel : nil, action: response.action)
                        .disabled(!response.enabled).accessibilityIdentifier(response.identifier)
                    .keyboardShortcut(response.cancel ? .cancelAction : response.accent ? .defaultAction : nil)
                }
            } message: { Text(message) }
        }
    }
}

private struct EditorAlertPresentation: ViewModifier {
    @ObservedObject var store: EditorStore
    let intent: JSON
    let title: String
    let message: String
    @Binding var presented: Bool
    let responses: [EditorAlertResponse]
    var bodyContent: AnyView? = nil
    private var phone: Bool { !store.snapshot["layout"]["phone"].isNull }
    private var custom: Bool { phone || bodyContent != nil }
    private var nativePresented: Binding<Bool> {
        Binding(get: { presented && !custom }, set: { if !custom { presented = $0 } })
    }
    func body(content: Content) -> some View {
        content.alert(title, isPresented: nativePresented) {
            ForEach(responses) { response in
                Button(response.label, role: response.destructive ? .destructive : response.cancel ? .cancel : nil, action: response.action)
                    .disabled(!response.enabled).accessibilityIdentifier(response.identifier)
                    .keyboardShortcut(response.cancel ? .cancelAction : response.accent ? .defaultAction : nil)
            }
        } message: { Text(message) }
            .modifier(EditorAlertSurface(store: store, presented: custom && presented, escape: {
                if let cancel = responses.first(where: { $0.cancel && $0.enabled }) { cancel.action(); return .handled }
                return .ignored
            }) { size in
                EditorAlertLayout(viewport: size, preferWide: intent["frame"]["prefer_wide"].bool,
                    fontPixels: CGFloat(store.catalog["text_size_pt"].number * 4 / 3)) {
                    Text(title).font(.headline).multilineTextAlignment(.center).accessibilityIdentifier("editor-alert-title")
                    ScrollView {
                        if let bodyContent { bodyContent.frame(maxWidth: .infinity) }
                        else { Text(message).multilineTextAlignment(.center).frame(maxWidth: .infinity) }
                    }
                    ForEach(responses) { response in
                        Button(role: response.destructive ? .destructive : response.cancel ? .cancel : nil, action: response.action) {
                            Text(response.label).multilineTextAlignment(.center).frame(maxWidth: .infinity)
                        }.buttonStyle(.bordered).tint(response.accent ? EditorPalette(source: store.state["palette"]).accent : nil)
                            .disabled(!response.enabled).accessibilityIdentifier(response.identifier)
                            .keyboardShortcut(response.cancel ? .cancelAction : response.accent ? .defaultAction : nil)
                    }
                }
            })
            .modifier(HostSurfacePresence(store: store, shown: presented))
    }
}

private struct EditorAlertSurface<Alert: View>: ViewModifier {
    @ObservedObject var store: EditorStore
    let presented: Bool
    var onDismiss: (() -> Void)? = nil
    var escape: () -> KeyPress.Result = { .ignored }
    @ViewBuilder let alert: (CGSize) -> Alert
    @State private var popup = UUID()
    @FocusState private var focused: Bool
    func body(content: Content) -> some View {
        content.disabled(presented).allowsHitTesting(!presented).accessibilityHidden(presented)
            .overlay {
                if presented {
                    GeometryReader { geometry in
                        let frame = geometry.frame(in: .global)
                        let height = max(0, store.snapshot["layout"]["viewport"][1].number - Double(store.obscuredBottom))
                        let visible = store.phoneVisibleArea
                            ?? CGRect(x: frame.minX, y: frame.minY, width: frame.width, height: min(frame.height, height))
                        let area = frame.intersection(visible)
                        ZStack {
                            Color.black.opacity(0.32).ignoresSafeArea().contentShape(Rectangle())
                            if !area.isNull {
                                alert(area.size)
                                    .background(EditorPalette(source: store.state["palette"])["settings"], in: SquircleShape.surface)
                                    .accessibilityAddTraits(.isModal)
                                    .modifier(EditorPopoverHost()).focusable().focused($focused)
                                    .onAppear { focused = true; store.workspace.popover(popup, open: true) }
                                    .onDisappear { store.workspace.popover(popup, open: false); onDismiss?() }
                                    .onNativeTextKey(.escape, action: escape)
                                    .position(x: area.midX - frame.minX, y: area.midY - frame.minY)
                            }
                        }.frame(width: geometry.size.width, height: geometry.size.height)
                    }
                }
            }
    }
}

struct EditorAlertLayout: Layout {
    let viewport: CGSize
    let preferWide: Bool
    let fontPixels: CGFloat
    private func geometry(_ subviews: Subviews) -> JSON {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        return DialogUI.resolve(["kind": "alert_layout", "viewport": [viewport.width, viewport.height],
            "metrics": ["minimum": 0, "natural": (sizes.prefix(2).map(\.width).max() ?? 0) + 48,
                "heading": sizes.first?.width ?? 0, "responses": sizes.dropFirst(2).map(\.width)],
            "font_pixels": fontPixels, "prefer_wide": preferWide])
    }
    private func measurements(_ subviews: Subviews, _ geometry: JSON) -> (CGFloat, [CGSize]) {
        let inset: CGFloat = geometry["short"].bool ? 18 : 24
        let inner = max(0, CGFloat(geometry["width"].number) - 2 * inset)
        let count = max(1, subviews.count - 2)
        let buttonWidth = geometry["stacked"].bool ? inner : max(0, (inner - 12 * CGFloat(count - 1)) / CGFloat(count))
        let buttons = subviews.dropFirst(2).map { $0.sizeThatFits(ProposedViewSize(width: buttonWidth, height: nil)) }
        let responses = geometry["stacked"].bool ? buttons.map(\.height).reduce(0, +) + 12 * CGFloat(max(0, count - 1)) : buttons.map(\.height).max() ?? 0
        let heading = subviews[0].sizeThatFits(ProposedViewSize(width: inner, height: nil))
        let idealBody = subviews[1].sizeThatFits(ProposedViewSize(width: inner, height: nil))
        let body = CGSize(width: inner, height: min(idealBody.height, max(0, viewport.height - inset * 2 - heading.height - responses - 24)))
        return (inset, [heading, body] + buttons)
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let geometry = geometry(subviews), (inset, sizes) = measurements(subviews, geometry)
        let responses = geometry["stacked"].bool ? sizes.dropFirst(2).map(\.height).reduce(0, +) + 12 * CGFloat(max(0, sizes.count - 3)) : sizes.dropFirst(2).map(\.height).max() ?? 0
        return CGSize(width: geometry["width"].number, height: inset * 2 + sizes[0].height + sizes[1].height + responses + 24)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let geometry = geometry(subviews), (inset, sizes) = measurements(subviews, geometry)
        let inner = max(0, bounds.width - inset * 2)
        var y = bounds.minY + inset
        for index in 0..<2 {
            subviews[index].place(at: CGPoint(x: bounds.minX + inset, y: y), proposal: ProposedViewSize(width: inner, height: sizes[index].height))
            y += sizes[index].height + 12
        }
        let indices = Array(2..<subviews.count)
        var x = bounds.minX + inset
        for index in geometry["stacked"].bool ? indices.reversed().map({ $0 }) : indices {
            let width = geometry["stacked"].bool ? inner : max(0, (inner - 12 * CGFloat(max(0, indices.count - 1))) / CGFloat(max(1, indices.count)))
            subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: width, height: sizes[index].height))
            if geometry["stacked"].bool { y += sizes[index].height + 12 } else { x += width + 12 }
        }
    }
}
