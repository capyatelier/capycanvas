import Foundation
import QuartzCore
import ImageIO
func decodedExportPixels(_ url: URL) throws -> Data {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let image = CGImageSourceCreateImageAtIndex(source, 0, nil), let pixels = image.dataProvider?.data else {
        throw HostFailure(message: "Could not decode the fixture's production export")
    }
    var result = Data("\(image.width):\(image.height):\(image.bitsPerPixel):\(image.bytesPerRow):".utf8)
    result.append(pixels as Data)
    return result
}

func navigationSave(_ value: JSON, to url: URL) async throws {
    try await Task.detached(priority: .utility) {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(value.encoded().utf8).write(to: url, options: .atomic)
    }.value
}

func storedTestFiles(_ root: URL) throws -> [String: Data] {
    var result: [String: Data] = [:]
    func visit(_ directory: URL) throws {
        for file in try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.isDirectoryKey]) {
            if try file.resourceValues(forKeys: [.isDirectoryKey]).isDirectory == true { try visit(file) }
            else if file.lastPathComponent != ".lock" { result[String(file.path.dropFirst(root.path.count + 1))] = try Data(contentsOf: file) }
        }
    }
    try visit(root)
    return result
}


func require(_ value: Bool, _ message: String) throws {
    if !value { throw HostFailure(message: message) }
}

@MainActor func wait(_ label: @autoclosure () -> String, seconds: TimeInterval = 45, failure: () -> String? = { nil },
    step: () async throws -> Void = {}, _ ready: () -> Bool) async throws {
    let deadline = Date().addingTimeInterval(seconds)
    while !ready() {
        if let message = failure() { throw HostFailure(message: message) }
        try require(Date() < deadline, "Timed out: \(label())")
        try await step()
        try await Task.sleep(for: .milliseconds(10))
    }
}

@MainActor func frame(_ native: NativeOwner) async {
    let now = FrameTrace.now()
    await withCheckedContinuation { done in native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
}

@MainActor func prepare(_ native: NativeOwner, _ label: String) async throws {
    let prepared = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
    try require(prepared, "Prepare canvas: \(label)")
}

@MainActor func attachSurface(_ store: EditorStore, _ size: CGSize) -> CAMetalLayer {
    let surface = CAMetalLayer()
    surface.bounds = CGRect(origin: .zero, size: size)
    store.native!.attach(surface, width: UInt32(size.width), height: UInt32(size.height), scale: 1)
    return surface
}

extension WorkspaceController {
    func started(_ label: String = "workspace startup") async throws {
        try await wait(label, failure: { self.error }) { self.ready && !self.busy }
    }
    func settle(_ label: String = "workspace operation") async throws {
        await withCheckedContinuation { done in
            store!.native!.workspace(JSON(["type": "tick"])) { _, _ in DispatchQueue.main.async { done.resume() } }
        }
        try await wait(label, failure: { self.busy ? nil : self.error }) { self.ready && !self.busy && !self.switcherBusy && !self.view["loading"].bool }
        try require(error == nil, "\(label): \(error ?? "")")
        try require(view["switcher_error"].isNull, view["switcher_error"].string)
    }
    func perform(_ input: [String: Any]) async throws { send(input); try await settle() }
    func answer(_ form: [String: Any], name: String? = nil, description: String? = nil, choice: String? = nil) async throws {
        try await perform(["type": "form", "action": form])
        try require(!view["prompt"].isNull, "No form for \(form)")
        if let name { formName = name }
        if let description { formDescription = description }
        if let choice { formChoice = choice }
        submit(); try await settle()
        try require(view["prompt"].isNull, "Form stayed open: \(error ?? view["prompt"].stableKey)")
    }
    func create(_ name: String) async throws { try await answer(["type": "new"], name: name) }
    func flushed() async throws {
        try await settle()
        try await wait("workspace autosave", failure: { self.error }) { !self.view["dirty"].bool && !self.view["saving"].bool }
    }
    func closed() async throws {
        let saved = await withCheckedContinuation { done in close { done.resume(returning: $0) } }
        try require(saved, error ?? "Workspace close failed")
    }
    func detached() async { await withCheckedContinuation { done in detach { done.resume() } } }
    func workspaceRows() async throws -> [JSON] {
        try await perform(["type": "open", "page": "workspaces"])
        let rows = view["rows"].array
        try await perform(["type": "dismiss"])
        return rows
    }
    func capture() async throws -> JSON {
        try await withCheckedThrowingContinuation { done in
            store!.native!.workspace(JSON(["type": "capture"])) { value, error in
                DispatchQueue.main.async {
                    if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume(returning: value ?? JSON()) }
                }
            }
        }
    }
}

func newSheet(_ extent: [Int], space: String = "Srgb", depth: String = "U8", transparent: Bool = false) -> [[String: Any]] {
    [["op": "unit", "unit": "px"], ["op": "width", "value": extent[0]], ["op": "height", "value": extent[1]],
     ["op": "space", "space": space], ["op": "depth", "depth": depth],
     ["op": "paper"], ["op": "paper_relief", "value": 0], ["op": "use_paper"], ["op": "paper_color"],
     ["op": "paper_color_value", "color": ["space": "Srgb", "rgba": [1, 1, 1, 1]]],
     ["op": "transparent", "transparent": transparent], ["op": "use_paper_color"]]
}

extension ProjectFiles {
    func newDrawingStep(_ action: [String: Any]) async throws {
        let error: String? = await withCheckedContinuation { done in newDrawing(action) { done.resume(returning: $0) } }
        if let error { throw HostFailure(message: error) }
    }
}

extension EditorStore {
    func orderNewDrawing(_ edits: [[String: Any]]) async throws {
        try await apply(["type": "invoke", "command": "new_document"])
        let steps: [[String: Any]] = [["op": "open", "screen": NSNull(), "untitled": 1]] + edits + [["op": "create"]]
        for step in steps { try await apply(["type": "new_drawing", "action": step]) }
    }
    func apply(_ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            edit(action) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
            }
        }
    }
}
