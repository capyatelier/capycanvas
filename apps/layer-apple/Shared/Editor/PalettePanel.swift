import SwiftUI

struct PalettePanel: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var controller: PaletteController
    let docked: Bool
    @StateObject private var grid = PaletteGridInteraction()
    @StateObject private var rows = PaletteRowInteraction()
    @State private var width: CGFloat = 264
    @State private var bodyHeight: CGFloat = 0
    @State private var popupID = UUID()
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var covered: Bool { controller.chooser || controller.expanded }
    private var hdr: Bool { store.snapshot["color_panel"]["hdr"].bool }
    var body: some View {
        let view = store.snapshot["palette_panel"]
        if !view.isNull {
            let cells = PaletteCells(width: max(PaletteCells.tile, width))
            let count = view["swatches"].array.count + 1
            VStack(spacing: 0) {
                ZStack(alignment: .topLeading) {
                    VStack(spacing: 0) {
                        PaletteHistory(controller: controller, view: view, cells: cells, rows: 1, expanded: false, enabled: !covered,
                            hdr: hdr, viewing: store.colorViewing)
                        divider
                        EditorScrollView(showsIndicators: false) {
                            swatchGrid(view, cells: cells)
                        }.frame(minHeight: cells.height(rows: 2), maxHeight: cells.viewport(count))
                            .accessibilityIdentifier("palette-swatches")
                            .modifier(PanelBodyMeasurement(panel: "palettes", part: "grid", intrinsicHeight: cells.viewport(count), kind: .scroll))
                            .modifier(PanelBodyMeasurement(panel: "palettes", part: "grid-unit", intrinsicHeight: PaletteCells.pitch, kind: .unit))
                    }.allowsHitTesting(!covered).accessibilityHidden(covered)
                        .modifier(PanelBodyMeasurement(panel: "palettes", part: "history", intrinsicHeight: PaletteCells.tile + 13))
                    if controller.expanded {
                        PaletteHistory(controller: controller, view: view, cells: cells,
                            rows: min(4, max(1, Int((bodyHeight + PaletteCells.gap) / PaletteCells.pitch))), expanded: true, enabled: true,
                            hdr: hdr, viewing: store.colorViewing)
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                            .background(palette["panel"]).accessibilityIdentifier("palette-history-expanded")
                    }
                    if controller.chooser {
                        PaletteChooser(store: store, controller: controller, view: view, rows: rows)
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                            .background(palette["panel"])
                    }
                }.frame(maxHeight: docked ? .infinity : nil, alignment: .top)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { bodyHeight = $0 }
                VStack(spacing: 0) {
                    divider
                    PaletteFooter(controller: controller, view: view)
                    if let message = controller.message {
                        Text(message.text).font(.system(size: 13))
                            .foregroundStyle(message.error ? Color.red : palette["text"].opacity(0.7))
                            .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                            .accessibilityIdentifier("palette-message")
                    }
                }.modifier(PanelBodyMeasurement(panel: "palettes", part: "footer"))
            }.onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
                .padding(.horizontal, 8).padding(.vertical, 6)
                .modifier(PanelBodyMeasurement(panel: "palettes", part: "padding", intrinsicHeight: 12))
                .accessibilityElement(children: .contain).accessibilityIdentifier("palette-panel")
                .onAppear { grid.store = store; rows.store = store; rows.owner = grid.owner }
                .onChange(of: controller.menu?.owner == grid.owner) { _, open in store.workspace.popover(popupID, open: open) }
                .onChange(of: store.snapshot["color_panel"]["definition"].stableKey) { _, _ in controller.retireStaleEdit() }
                .onChange(of: view["palettes"].stableKey) { _, _ in controller.retireStaleEdit() }
                .onChange(of: view["swatches"].stableKey) { _, _ in grid.validate() }
                .onDisappear {
                    grid.cancel(); rows.cancel(); controller.closeMenu(owner: grid.owner)
                    store.workspace.popover(popupID, open: false)
                }
        }
    }
    private var divider: some View {
        Rectangle().fill(palette["text"].opacity(0.15)).frame(height: 1).padding(.vertical, 6)
    }
    private func swatchGrid(_ view: JSON, cells: PaletteCells) -> some View {
        let swatches = view["swatches"].array, ids = swatches.map { $0["id"].uint }
        let order = grid.drag?.order ?? controller.settle.flatMap { $0 == ids ? nil : $0 }
        let selected = controller.selection(view)?["id"].uint
        let count = swatches.count + 1
        grid.cells = cells; grid.swatches = swatches; grid.palette = view["palette"].uint
        grid.covered = covered; grid.selected = selected
        return ZStack(alignment: .topLeading) {
            ForEach(Array(swatches.enumerated()), id: \.element.paletteID) { index, swatch in
                let id = swatch["id"].uint
                let target = order?.firstIndex(of: id) ?? index
                PaletteSwatchTile(swatch: swatch, selected: selected == id, lifted: controller.lift?.id == id && controller.lift?.owner == grid.owner,
                    dragging: controller.lift != nil, hdr: hdr, viewing: store.colorViewing) {
                    guard !grid.contact.consumeClick() else { return }
                    controller.use(id)
                }
                .frame(width: cells.width(index), height: PaletteCells.tile)
                .offset(x: cells.x(target), y: cells.y(target))
                .animation(PaletteSlide.animation, value: target)
                .editorPopover(isPresented: controller.menuPresented(.color(id), owner: grid.owner), placement: .inward) { menu }
            }
            Button {
                controller.focused = true; controller.addCurrent()
            } label: {
                SharedIcon(name: "plus").frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(palette["input"], in: SquircleShape.control).contentShape(Rectangle())
            }.buttonStyle(.plain).disabled(!view["can_name"].bool || covered).opacity(view["can_name"].bool ? 1 : 0.4)
                .frame(width: cells.width(count - 1), height: PaletteCells.tile)
                .offset(x: cells.x(count - 1), y: cells.y(count - 1))
                .help("Add current color to this palette").accessibilityLabel("Add current color to this palette")
                .accessibilityIdentifier("palette-add-color")
        }.frame(width: cells.width, height: cells.height(rows: cells.rows(count)), alignment: .topLeading)
            .background(NativeReorderInput(model: grid))
            .onGeometryChange(for: CGPoint.self) { $0.frame(in: .named("editor-workspace")).origin } action: { grid.origin = $0 }
    }
    private var menu: some View {
        EditorActionMenu(model: AppleContextMenu(controller.menu?.model ?? JSON(), invoke: { store.dispatch($0) },
            command: { controller.command($0) }), identifier: "palette-context-menu") { controller.closeMenu() }
    }
}

enum PaletteSlide {
    @MainActor static var animation: Animation? {
        #if os(macOS)
        NSWorkspace.shared.accessibilityDisplayShouldReduceMotion ? nil : .timingCurve(0.33, 1, 0.68, 1, duration: 0.14)
        #else
        UIAccessibility.isReduceMotionEnabled ? nil : .timingCurve(0.33, 1, 0.68, 1, duration: 0.14)
        #endif
    }
}

private struct PaletteSwatchTile: View {
    let swatch: JSON
    let selected: Bool
    let lifted: Bool
    let dragging: Bool
    let hdr: Bool
    let viewing: JSON
    let use: () -> Void
    @State private var hovering = false
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let detail = swatch["detail"].string
        Button(action: use) {
            PaletteFace(rgba: swatch["rgba"], color: swatch["color"], hdr: hdr, viewing: viewing)
                .padding(3).frame(maxWidth: .infinity, maxHeight: .infinity)
                .background { if hovering && !dragging { SquircleShape.control.fill(palette["text"].opacity(0.1)) } }
                .overlay { if selected { SquircleShape.control.strokeBorder(palette.accent, lineWidth: 2) } }
                .contentShape(Rectangle())
        }.buttonStyle(.plain).opacity(lifted ? 0 : 1)
            .onHover { hovering = $0 }
            .help(dragging ? "" : detail).accessibilityLabel(detail)
            .accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityIdentifier("palette-swatch-\(swatch["id"].uint)")
    }
}

struct PaletteFace: View {
    let rgba: JSON
    let color: JSON
    let hdr: Bool
    let viewing: JSON
    var shape = SquircleShape.control
    var body: some View {
        if hdr { HDRColorSwatch(color: color, viewing: viewing, shape: shape) }
        else { ColorSwatch(rgba: rgba, shape: shape) }
    }
}

private struct PaletteHistory: View {
    @ObservedObject var controller: PaletteController
    let view: JSON
    let cells: PaletteCells
    let rows: Int
    let expanded: Bool
    let enabled: Bool
    let hdr: Bool
    let viewing: JSON
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let history = view["history"].array, capacity = cells.columns * rows
        ZStack(alignment: .topLeading) {
            ForEach(0..<max(0, capacity - 1), id: \.self) { index in
                Group {
                    if index < history.count {
                        PaletteRecentTile(controller: controller, tile: history[index], enabled: enabled, hdr: hdr, viewing: viewing)
                    } else if history.isEmpty && index < 5 {
                        SquircleShape.control.fill(palette["text"].opacity(0.05)).padding(3)
                            .accessibilityElement().accessibilityLabel("Colors appear here after painting")
                            .accessibilityIdentifier("palette-empty-\(index)")
                    }
                }.frame(width: cells.width(index), height: PaletteCells.tile).offset(x: cells.x(index), y: cells.y(index))
            }
            Button {
                controller.focused = true; controller.chooser = false; controller.expanded = !expanded
            } label: {
                SharedIcon(name: "chevron-down").rotationEffect(.degrees(expanded ? 180 : 0))
                    .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).disabled(!enabled)
                .frame(width: cells.width(capacity - 1), height: PaletteCells.tile)
                .offset(x: cells.x(capacity - 1), y: cells.y(capacity - 1))
                .help(expanded ? "Collapse color history" : "Expand color history")
                .accessibilityLabel(expanded ? "Collapse color history" : "Expand color history")
                .accessibilityIdentifier(expanded ? "palette-history-collapse" : "palette-history-expand")
        }.frame(width: cells.width, height: cells.height(rows: rows), alignment: .topLeading)
            .accessibilityElement(children: .contain).accessibilityIdentifier(expanded ? "palette-history-grid" : "palette-history")
    }
}

private struct PaletteRecentTile: View {
    @ObservedObject var controller: PaletteController
    let tile: JSON
    let enabled: Bool
    let hdr: Bool
    let viewing: JSON
    @State private var hovering = false
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let detail = tile["detail"].string
        Button { controller.useRecent(tile["color"]) } label: {
            PaletteFace(rgba: tile["rgba"], color: tile["color"], hdr: hdr, viewing: viewing)
                .padding(3).frame(maxWidth: .infinity, maxHeight: .infinity)
                .background { if hovering && controller.lift == nil { SquircleShape.control.fill(palette["text"].opacity(0.1)) } }
                .contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(!enabled).onHover { hovering = $0 }
            .help(detail).accessibilityLabel(detail).accessibilityIdentifier("palette-recent-color")
    }
}

private struct PaletteFooter: View {
    @ObservedObject var controller: PaletteController
    let view: JSON
    var measuring = false
    @Environment(\.editorPalette) private var palette
    @FocusState private var editorFocused: Bool
    var body: some View {
        let selection = controller.selection(view)
        let name = selection?["name"].string ?? view["color_name"].string
        HStack(spacing: 4) {
            Button { controller.focused = true; controller.toggleChooser() } label: {
                HStack(spacing: 4) {
                    Text(view["name"].string).lineLimit(1).truncationMode(.tail)
                    SharedIcon(name: "chevron-down", size: 12).rotationEffect(.degrees(180))
                }.padding(.horizontal, 4).frame(minHeight: 24).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).frame(maxWidth: 150, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                .help("Choose a palette · " + view["name"].string).accessibilityLabel("Choose a palette")
                .accessibilityValue(view["name"].string).accessibilityIdentifier("palette-chooser")
            Spacer(minLength: 0)
            VStack(alignment: .trailing, spacing: 0) {
                if controller.editing && !measuring {
                    TextField("Color name", text: Binding(get: { controller.editText }, set: { controller.editText = String($0.prefix(64)) }))
                        .textFieldStyle(.plain).multilineTextAlignment(.trailing)
                        .padding(.horizontal, 6).padding(.vertical, 2)
                        .frame(minWidth: 64, maxWidth: 180).frame(height: 24)
                        .background(palette["input"], in: SquircleShape.control)
                        .overlay { if controller.message?.error == true { SquircleShape.control.strokeBorder(Color(red: 0.93, green: 0.33, blue: 0.33), lineWidth: 1) } }
                        .focused($editorFocused)
                        .onAppear { DispatchQueue.main.async { editorFocused = true } }
                        .onSubmit { controller.commitName(controller.editText) }
                        .onKeyPress(.escape) { controller.cancelEditing(); return .handled }
                        .onChange(of: editorFocused) { _, focused in if !focused { controller.commitName(controller.editText) } }
                        .accessibilityIdentifier("palette-name-editor")
                } else {
                    Button { controller.focused = true; controller.beginEditing() } label: {
                        Text(name).lineLimit(1).padding(.horizontal, 2).frame(minHeight: 24).contentShape(Rectangle())
                    }.buttonStyle(.plain).disabled(!view["can_name"].bool).opacity(view["can_name"].bool ? 1 : 0.4)
                        .help(name + " · Click to rename").accessibilityLabel(name).accessibilityIdentifier("palette-color-name")
                }
                Text(view["color_detail"].string).font(.system(size: 12)).foregroundStyle(palette["text"].opacity(0.6))
                    .lineLimit(1).padding(.trailing, 2)
                    .help("sRGB hex preview; saved colors retain their original color space, alpha and HDR intensity")
                    .accessibilityIdentifier("palette-color-detail")
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("palette-footer")
    }
}

private struct PaletteChooser: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var controller: PaletteController
    let view: JSON
    @ObservedObject var rows: PaletteRowInteraction
    @State private var query = ""
    @FocusState private var searching: Bool
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let needle = query.trimmingCharacters(in: .whitespaces).lowercased()
        let matches = view["palettes"].array.filter { needle.isEmpty || $0["name"].string.lowercased().contains(needle) }
        VStack(spacing: 6) {
            HStack(spacing: 4) {
                HStack(spacing: 6) {
                    SharedIcon(name: "search", size: 14).opacity(0.6)
                    TextField("Find a palette", text: $query).textFieldStyle(.plain).focused($searching)
                        .onKeyPress(.escape) { controller.chooser = false; return .handled }
                        .accessibilityIdentifier("palette-search")
                }.padding(.horizontal, 6).frame(height: 24).background(palette["input"], in: SquircleShape.control)
                Button { controller.openMenu(.library, owner: rows.owner) } label: {
                    SharedIcon(name: "plus").frame(width: 24, height: 24).contentShape(Rectangle())
                }.buttonStyle(EditorControlButtonStyle()).help("New or import palette").accessibilityLabel("New or import palette")
                    .accessibilityIdentifier("palette-library-add")
                    .editorPopover(isPresented: controller.menuPresented(.library, owner: rows.owner), placement: .inward) { menu }
            }
            ZStack {
                EditorScrollView(showsIndicators: false) {
                    LazyVStack(spacing: 0) {
                        ForEach(matches, id: \.paletteID) { row in PaletteChoiceRow(controller: controller, row: row, rows: rows, menu: menu) }
                    }.coordinateSpace(name: "palette-rows")
                        .background(NativeReorderInput(model: rows))
                        .onPreferenceChange(PaletteRowFrames.self) { rows.frames = $0 }
                }.accessibilityIdentifier("palette-list")
                if matches.isEmpty {
                    Text("No matching palettes").foregroundStyle(palette["text"].opacity(0.6)).accessibilityIdentifier("palette-empty-search")
                }
            }.frame(maxHeight: .infinity)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("palette-browser")
            .onAppear {
                #if os(macOS)
                searching = true
                #endif
            }
    }
    private var menu: some View {
        EditorActionMenu(model: AppleContextMenu(controller.menu?.model ?? JSON(), invoke: { store.dispatch($0) },
            command: { controller.command($0) }), identifier: "palette-context-menu") { controller.closeMenu() }
    }
}

private struct PaletteChoiceRow<Menu: View>: View {
    @ObservedObject var controller: PaletteController
    let row: JSON
    let rows: PaletteRowInteraction
    let menu: Menu
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let id = row["id"].uint, active = row["active"].bool
        Button {
            guard !rows.contact.consumeClick() else { return }
            controller.choose(id)
        } label: {
            HStack(spacing: 6) {
                Text(row["name"].string).lineLimit(1).truncationMode(.tail).frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: 0) {
                    ForEach(row["preview"].array.indices, id: \.self) { index in
                        ColorSwatch(rgba: row["preview"][index]).frame(width: 12, height: 18)
                    }
                }.clipShape(RoundedRectangle(cornerRadius: 3)).accessibilityIdentifier("palette-preview-\(id)")
            }.padding(4).frame(minHeight: 32)
                .background(active ? palette.active : Color.clear, in: SquircleShape.control).contentShape(Rectangle())
        }.buttonStyle(.plain)
            .background(GeometryReader { geometry in
                Color.clear.preference(key: PaletteRowFrames.self, value: [id: geometry.frame(in: .named("palette-rows"))])
            })
            .accessibilityLabel(row["name"].string).accessibilityAddTraits(active ? .isSelected : [])
            .accessibilityIdentifier("palette-choice-\(id)")
            .editorPopover(isPresented: controller.menuPresented(.palette(id), owner: rows.owner), placement: .inward) { menu }
    }
}

private extension JSON { var paletteID: UInt64 { self["id"].uint } }

struct PaletteMeasurement: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var controller: PaletteController
    @Environment(\.editorPalette) private var palette
    var body: some View {
        let view = store.snapshot["palette_panel"]
        if !view.isNull {
            VStack(spacing: 0) {
                GeometryReader { geometry in
                    let cells = PaletteCells(width: max(PaletteCells.tile, geometry.size.width - 16))
                    Color.clear
                        .modifier(PanelBodyMeasurement(panel: "palettes", part: "history", intrinsicHeight: PaletteCells.tile + 13))
                        .modifier(PanelBodyMeasurement(panel: "palettes", part: "grid",
                            intrinsicHeight: cells.viewport(view["swatches"].array.count + 1), kind: .scroll))
                        .modifier(PanelBodyMeasurement(panel: "palettes", part: "grid-unit", intrinsicHeight: PaletteCells.pitch, kind: .unit))
                        .modifier(PanelBodyMeasurement(panel: "palettes", part: "padding", intrinsicHeight: 12))
                }.frame(height: 1)
                VStack(spacing: 0) {
                    Rectangle().frame(height: 1).padding(.vertical, 6)
                    PaletteFooter(controller: controller, view: view, measuring: true)
                }.fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
                    .modifier(PanelBodyMeasurement(panel: "palettes", part: "footer"))
            }.opacity(0).allowsHitTesting(false).accessibilityHidden(true)
        }
    }
}

struct PaletteDragOverlay: View {
    @ObservedObject var controller: PaletteController
    let hdr: Bool
    let viewing: JSON
    @Environment(\.editorPalette) private var palette
    var body: some View {
        if let lift = controller.lift {
            PaletteFace(rgba: lift.rgba, color: lift.color, hdr: hdr, viewing: viewing)
                .padding(3).frame(width: lift.frame.width, height: lift.frame.height)
                .background(palette["panel"], in: SquircleShape.control)
                .overlay { if lift.selected { SquircleShape.control.strokeBorder(palette.accent, lineWidth: 2) } }
                .shadow(color: .black.opacity(0.35), radius: 4, y: 3)
                .offset(x: lift.frame.minX, y: lift.frame.minY)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .allowsHitTesting(false).accessibilityIdentifier("palette-drag-preview")
        }
    }
}
