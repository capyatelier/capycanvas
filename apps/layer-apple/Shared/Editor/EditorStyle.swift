import SwiftUI
import ImageIO
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

struct EditorPalette {
    let source: JSON
    var onGlass = false
    subscript(_ name: String) -> Color { Color(hex: source[name].string) }
    private func role(_ name: String, _ fallback: String) -> Color { Color(hex: source[name].isNull ? fallback : source[name].string) }
    private func rgba(_ value: JSON) -> Color? {
        let channels = value.array
        guard channels.count == 4 else { return nil }
        return Color(.sRGB, red: channels[0].number, green: channels[1].number, blue: channels[2].number, opacity: channels[3].number)
    }
    private func glass(_ name: String, _ fallback: @autoclosure () -> Color) -> Color { rgba(source["glass"][name]) ?? fallback() }
    private func line(_ value: JSON) -> Color { rgba(value) ?? .clear }
    var settingsCardShade: Color { line(source["lines"]["settings"]["card_shade"]) }
    var settingsBorder: Color { line(source["lines"]["settings"]["border"]) }
    var panelBorder: Color { line(source["lines"]["panel"]["border"]) }
    var popoverBorder: Color { line(source["lines"]["popover_border"]) }
    var controlFill: Color { line(source["control_fill"]) }
    var field: Color { self["text"].opacity(0.10) }
    var accent: Color { role("accent", "#3584e4") }
    var accentForeground: Color { role("accent_foreground", "#ffffff") }
    var active: Color { onGlass ? glass("selection", role("selection", "#c0d7f6")) : role("selection", "#c0d7f6") }
    var headerSelection: Color { glass("header_selection", role("header_selection", "#afc6e5")) }
    var headerSelectionHover: Color { role("header_selection_hover", "#a6bddb") }
    var chromeSurface: Color { glass("chip", self["bg"]) }
    var glassPanel: Color { glass("panel", self["panel"]) }
    var glassStrip: Color { glass("strip", self["tabbar"]) }
    var glassSwitcher: Color { glass("switcher", self["tabbar"]) }
    var glassSwitcherSelection: Color { glass("switcher_selection", role("header_selection", "#afc6e5")) }
    var glassDocumentTab: Color { glass("document_tab", self["panel"]) }
    var glassy: EditorPalette { var palette = self; palette.onGlass = true; return palette }
    var checkerLight: Color { role("checker_light", "#ffffff") }
    var checkerDark: Color { role("checker_dark", "#cccccc") }
}
extension GraphicsContext {
    func fillTransparencyChecker(_ size: CGSize, palette: EditorPalette) {
        let cell: CGFloat = 5
        for row in 0..<Int(ceil(size.height / cell)) {
            for column in 0..<Int(ceil(size.width / cell)) {
                fill(Path(CGRect(x: CGFloat(column) * cell, y: CGFloat(row) * cell, width: cell, height: cell)),
                    with: .color((row + column) % 2 == 0 ? palette.checkerLight : palette.checkerDark))
            }
        }
    }
}
private struct EditorPaletteKey: EnvironmentKey { static let defaultValue = EditorPalette(source: JSON()) }
extension EnvironmentValues {
    var editorPalette: EditorPalette {
        get { self[EditorPaletteKey.self] }
        set { self[EditorPaletteKey.self] = newValue }
    }
}
extension Color {
    init(hex: String) {
        let value = UInt64(hex.trimmingCharacters(in: CharacterSet(charactersIn: "#")), radix: 16) ?? 0x333333
        self.init(.sRGB, red: Double((value >> 16) & 255) / 255,
            green: Double((value >> 8) & 255) / 255, blue: Double(value & 255) / 255)
    }
}
extension JSON {
    var rect: CGRect { CGRect(x: self["x"].number, y: self["y"].number, width: self["width"].number, height: self["height"].number) }
}
extension View {
    func placed(_ bounds: JSON) -> some View {
        let r = bounds.rect
        return frame(width: max(0, r.width), height: max(0, r.height), alignment: .topLeading).offset(x: r.minX, y: r.minY)
    }
}
struct EditorLine: View {
    let color: Color
    var body: some View { Rectangle().fill(color).frame(height: 1).accessibilityHidden(true) }
}

/// Shared grip orientation and inset for panel headers and column footers.
struct PanelGrip: View {
    var vertical = false
    var body: some View {
        SharedIcon(name: "grip").opacity(0.65)
            .rotationEffect(.degrees(vertical ? 90 : 0))
            .offset(x: vertical ? 0 : -2, y: vertical ? -2 : 0)
    }
}

struct SharedIcon: View {
    let name: String
    var size: CGFloat = 16
    var body: some View {
        let key = Self.assetKey(name)
        Group {
            if let layers = Self.layers[key] {
                ZStack {
                    ForEach(layers, id: \.asset) { layer in
                        glyph(layer.asset, template: layer.template)
                    }
                }.compositingGroup()
            } else {
                glyph("icon-" + key, template: true)
            }
        }.accessibilityHidden(true)
    }
    static func assetKey(_ name: String) -> String {
        var key = name
        if key.hasPrefix("layer-") { key.removeFirst("layer-".count) }
        if key.hasSuffix("-symbolic") { key.removeLast("-symbolic".count) }
        return key
    }
    private func glyph(_ asset: String, template: Bool) -> some View {
        Image(asset).resizable().renderingMode(template ? .template : .original)
            .scaledToFit().frame(width: size, height: size)
    }
    private struct Layer: Decodable { let asset: String; let template: Bool }
    // The generated data asset shares Assets.car with the vectors on both hosts.
    // Load once; fixed paints and foreground masks retain SVG painter order.
    private static let layers: [String: [Layer]] = {
        guard let data = NSDataAsset(name: "shared-icon-paints")?.data else { return [:] }
        return (try? JSONDecoder().decode([String: [Layer]].self, from: data)) ?? [:]
    }()
}
struct BundledPreview: View {
    let name: String
    @State private var image: CGImage?
    var body: some View {
        ZStack {
            if let image = Self.images.object(forKey: name as NSString) ?? image { Image(decorative: image, scale: 1).resizable() }
        }.task(id: name) {
            let name = name
            image = await Task.detached(priority: .userInitiated) { SliderStampImage(image: Self.decode(name)) }.value.image
        }
    }
    nonisolated(unsafe) private static let images: NSCache<NSString, CGImage> = {
        let cache = NSCache<NSString, CGImage>()
        cache.totalCostLimit = 8 << 20
        return cache
    }()
    nonisolated private static func decode(_ name: String) -> CGImage? {
        if let image = images.object(forKey: name as NSString) { return image }
        guard let data = NSDataAsset(name: name)?.data, let source = CGImageSourceCreateWithData(data as CFData, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary) else { return nil }
        images.setObject(image, forKey: name as NSString, cost: image.bytesPerRow * image.height)
        return image
    }
}
struct IconTile: View {
    let icon: String
    let label: String
    var selected = false
    var enabled = true
    var size: CGFloat = 16
    var active = false
    var joinedEdge: String?
    var background: Color?
    var keepsBackground = false
    var drawerBackground: Color?
    var corner = SquircleShape.Corner.radius(SquircleShape.controlRadius)
    let action: () -> Void
    var body: some View {
        Button(action: action) { PanelIcon(name: icon, size: size).frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            .buttonStyle(EditorControlButtonStyle(selected: selected, active: active, joinedEdge: joinedEdge,
                background: background, keepsBackground: keepsBackground, drawerBackground: drawerBackground, corner: corner))
            .disabled(!enabled).opacity(enabled ? 1 : 0.36)
            .accessibilityLabel(label).help(label)
            .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// One shared disabled-opacity step is applied by the caller to the whole
/// control, including its selection. PlainButtonStyle would dim the glyph again.
struct EditorControlButtonStyle: PrimitiveButtonStyle {
    var selected = false
    var active = false
    var joinedEdge: String?
    var background: Color?
    var keepsBackground = false
    var drawerBackground: Color?
    var corner = SquircleShape.Corner.radius(SquircleShape.controlRadius)
    private var shape: SquircleShape { SquircleShape.joined(joinedEdge, corner: corner) }
    func makeBody(configuration: Configuration) -> some View { Control(configuration: configuration, style: self) }
    private struct Control: View {
        let configuration: PrimitiveButtonStyleConfiguration
        let style: EditorControlButtonStyle
        @Environment(\.bubbleEdits) private var edits
        var body: some View {
            Button(role: configuration.role) {
                guard edits?.consumeClick() != true else { return }
                configuration.trigger()
            } label: { configuration.label }
                .buttonStyle(VisualStyle(style: style))
        }
    }
    private struct VisualStyle: ButtonStyle {
        let style: EditorControlButtonStyle
        func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, style: style) }
    }
    private struct Face: View {
        let configuration: ButtonStyleConfiguration
        let style: EditorControlButtonStyle
        @Environment(\.editorPalette) private var palette
        var body: some View {
            configuration.label.background {
                ZStack {
                    if let background = style.background, style.keepsBackground || !(style.selected || configuration.isPressed || style.active) {
                        style.shape.fill(background)
                    }
                    if style.selected {
                        style.shape.fill(palette.active)
                    } else if let drawerBackground = style.drawerBackground {
                        style.shape.fill(drawerBackground)
                    } else if configuration.isPressed || style.active {
                        style.shape.fill(.foreground).opacity(configuration.isPressed ? 0.16 : 0.10)
                    }
                }
            }.modifier(BubbleNativeControl(drags: false))
        }
    }
}

/// Title-bar controls: one translucent surface, or none inside a joined bar,
/// with full-height feedback in the bar and the drawer origin filling the tile.
struct HeaderButtonStyle: ButtonStyle {
    var selected = false
    var hovering = false
    var inBar = false
    var drawerOpen = false
    let radius: CGFloat
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, style: self) }
    private struct Face: View {
        let configuration: Configuration
        let style: HeaderButtonStyle
        @Environment(\.editorPalette) private var palette
        var body: some View {
            let r = style.radius
            let shape = style.drawerOpen ? SquircleShape(topLeading: r, topTrailing: r) : SquircleShape(r)
            configuration.label.background {
                ZStack {
                    if !style.inBar { shape.fill(style.drawerOpen && !style.selected ? palette.glassPanel : palette.chromeSurface) }
                    Group {
                        if style.selected {
                            shape.fill(palette.headerSelection)
                        } else if style.drawerOpen {
                            if style.inBar { shape.fill(palette.glassPanel) }
                        } else if configuration.isPressed || style.hovering {
                            shape.fill(.foreground).opacity(configuration.isPressed ? 0.16 : 0.10)
                        }
                    }
                }
            }.modifier(GlassRegistration(shape: shape, active: !style.inBar))
        }
    }
}

@MainActor struct ToolCommandToggle: View {
    static let intrinsicSize: CGSize = {
        #if os(macOS)
        return NSButton(checkboxWithTitle: "", target: nil, action: nil).fittingSize
        #else
        return UISwitch().intrinsicContentSize
        #endif
    }()
    let command: JSON
    let action: () -> Void
    var body: some View {
        Toggle(command["label"].string, isOn: Binding(get: { command["selected"].bool }, set: { _ in action() }))
            #if os(macOS)
            .toggleStyle(.checkbox)
            #else
            .toggleStyle(.switch).controlSize(.mini)
            #endif
            .lineLimit(1).fixedSize().frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct ToolActionControl: View {
    let command: JSON
    let checkable: Bool
    let side: CGFloat
    var textSize: CGFloat = 44 / 3
    var caption: String? = nil
    var iconOnly = false
    let action: () -> Void
    private var selected: Bool { checkable && command["selected"].bool }
    var body: some View {
        Group {
            if checkable {
                Toggle(command["label"].string, isOn: Binding(get: { selected }, set: { _ in action() }))
                    .toggleStyle(PanelCheckStyle(minHeight: side))
            } else {
                Button(action: action) {
                    HStack(spacing: 4) {
                        if !command["icon"].string.isEmpty { SharedIcon(name: command["icon"].string, size: 16) }
                        if !iconOnly { Text(caption ?? command["label"].string).lineLimit(1).truncationMode(.tail) }
                    }.frame(width: iconOnly ? side : nil)
                        .frame(maxWidth: iconOnly ? nil : .infinity, minHeight: side, alignment: iconOnly ? .center : .leading)
                        .padding(.horizontal, iconOnly ? 0 : 6).contentShape(Rectangle())
                }.buttonStyle(EditorControlButtonStyle(selected: selected))
                    .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }.font(.system(size:textSize)).disabled(!command["enabled"].bool).opacity(command["enabled"].bool ? 1 : 0.36)
            .help(command["tooltip"].string).accessibilityLabel(command["label"].string)
            .accessibilityIdentifier("tool-action-" + command["id"].string)
    }
}

struct PanelCheckStyle: ToggleStyle {
    let minHeight: CGFloat
    @Environment(\.bubbleEdits) private var edits
    func makeBody(configuration: Configuration) -> some View {
        Button { if edits?.consumeClick() != true { configuration.isOn.toggle() } } label: {
            HStack(spacing: 4) {
                ZStack {
                    SquircleShape(4).stroke(.primary.opacity(0.35), lineWidth: 2)
                    if configuration.isOn { SharedIcon(name: "check", size: 16) }
                }.frame(width: 16, height: 16).frame(width: 18)
                configuration.label.lineLimit(1)
            }.frame(minHeight: minHeight).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityAddTraits(configuration.isOn ? .isSelected : [])
            .modifier(BubbleNativeControl(drags: false))
    }
}

struct PanelControlMetrics {
    let view: JSON
    var inset: CGFloat { CGFloat(view["inset"].number) }
    var spacing: CGFloat { CGFloat(view["control_spacing"].number) }
    var group: CGFloat { CGFloat(view["group_spacing"].number) }
    var compound: CGFloat { CGFloat(view["compound_spacing"].number) }
    var height: CGFloat { CGFloat(view["control_height"].number) }
    var radius: CGFloat { CGFloat(view["control_radius"].number) }
    var sliderHeight: CGFloat { CGFloat(view["slider_height"].number) }
    var padding: CGFloat { CGFloat(view["field_padding"].number) }
    var icon: CGFloat { CGFloat(view["icon_size"].number) }
}
extension EditorStore {
    var panelMetrics: PanelControlMetrics { PanelControlMetrics(view: catalog["panel_metrics"]) }
    func panelLabelWidth(_ labels: [String]) -> CGFloat {
        min(120, labels.map { EditorTextMetrics.width($0, size: catalog["text_size_pt"].number * 4 / 3, weight: .regular) }.max() ?? 0)
    }
}
private struct CompactPanelKey: EnvironmentKey { static let defaultValue = false }
private struct SettingsSurfaceKey: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var compactPanelControls: Bool {
        get { self[CompactPanelKey.self] }
        set { self[CompactPanelKey.self] = newValue }
    }
    var onSettingsSurface: Bool {
        get { self[SettingsSurfaceKey.self] }
        set { self[SettingsSurfaceKey.self] = newValue }
    }
}

/// Local drafts keep text/caret stable while the serial owner publishes edits.
struct EditorTextField: View {
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
        TextField(label, text: Binding(get: { text }, set: { next in
            // Native fields can resend their value when editing ends.
            guard next != text else { return }
            text = next; pending = next; edit(next)
        }))
            .onChange(of: value) { _, next in
                if pending == nil || pending == next { text = next; pending = nil }
            }
    }
}

extension View {
    func editorSearchInput() -> some View {
        #if os(iOS)
        textInputAutocapitalization(.never).autocorrectionDisabled()
        #else
        autocorrectionDisabled()
        #endif
    }
}

/// Submit the focused field before a containing dialog closes.
private struct EditorTextCommit: FocusedValueKey { typealias Value = () -> Void }
extension FocusedValues {
    var editorTextCommit: (() -> Void)? {
        get { self[EditorTextCommit.self] }
        set { self[EditorTextCommit.self] = newValue }
    }
}
