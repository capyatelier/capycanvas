import SwiftUI

/// One immutable delivery copy. Shared validation, CMM work and previews run on I/O.
@MainActor final class ExportController: ObservableObject {
    @Published private(set) var busy = false
    @Published private(set) var loaded = false
    @Published private(set) var choiceRevision = 0
    @Published private(set) var details = JSON()
    @Published private(set) var draft = JSON()
    var recipe: JSON { draft["recipe"] }
    @Published private(set) var profiles: [JSON] = []
    @Published private(set) var profileIndex = 0
    @Published private(set) var names: [String] = []
    @Published private(set) var destination = 0
    @Published private(set) var previews: [CGImage] = []
    @Published private(set) var clipped: UInt64 = 0
    @Published var error: String?
    let preferences: ColorPreferencesStore
    private weak var native: NativeOwner?
    private let request: UInt64
    private var task: NativeProjectTask?
    private var closing = false
    private var finished = false
    private let completion: (NativeProjectTask?, JSON, Int, JSON) -> Void
    init(native: NativeOwner, request: UInt64, preferences: ColorPreferencesStore,
        completion: @escaping (NativeProjectTask?, JSON, Int, JSON) -> Void) {
        self.native = native; self.request = request; self.preferences = preferences; self.completion = completion
    }
    func load(ready: (() -> Void)? = nil) {
        guard !busy, !closing, !finished, let native else { return }
        busy = true; error = nil
        native.exportTask(id: request) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self else { task?.cancel(); return }
                if self.closing { task?.cancel(); self.busy = false; self.finish(nil); return }
                guard let task else { self.failed(error ?? "The drawing is unavailable"); return }
                self.task = task
                let preferences = self.preferences
                NativeProjectTask.io.async { [weak self] in
                    do {
                        let details = try task.details()
                        let view = try preferences.presets(color: details["color"], request: JSON(["type": "get", "index": 0]))
                        let profile = ExportController.matchProfile(view["recipe"]["profile"], in: details["form"]["profiles"].array)
                        let draft = try preferences.exportDraft(recipe: view["recipe"])
                        DispatchQueue.main.async { [weak self] in
                            guard let self else { return }
                            self.busy = false
                            if self.closing { self.finish(nil); return }
                            self.details = details; self.accept(view, draft: draft, profiles: profile.0, selected: profile.1)
                            self.loaded = true; ready?()
                        }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { [weak self] in self?.failed(message) }
                    }
                }
            }
        }
    }
    private nonisolated static func matchProfile(_ profile: JSON, in values: [JSON]) -> ([JSON], Int) {
        // ICC byte equality belongs to the worker, never a SwiftUI body update.
        if let index = values.firstIndex(where: { ($0["profile"].raw as? NSDictionary)?.isEqual(profile["profile"].raw) == true }) {
            return (values, index)
        }
        return (values + [profile], values.count)
    }
    private func accept(_ view: JSON, draft: JSON?, profiles: [JSON], selected: Int) {
        names = view["names"].array.map(\.string)
        if !view["index"].isNull { destination = Int(view["index"].uint) }
        if let draft {
            self.draft = draft; self.profiles = profiles; profileIndex = selected
        }
        choiceRevision += 1; invalidate()
    }
    func invalidate() { previews = []; clipped = 0; error = nil }
    func change(_ key: String, _ value: JSON) {
        guard loaded, !busy, !closing, !finished else { return }
        busy = true; invalidate()
        let recipe = recipe, preferences = preferences, profiles = profiles
        NativeProjectTask.io.async { [weak self] in
            do {
                let draft = try preferences.exportDraft(recipe: recipe, action: JSON(["type": key, "value": value.raw]))
                let selected = key == "profile" ? ExportController.matchProfile(value, in: profiles) : nil
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.busy = false
                    if self.closing { self.finish(nil); return }
                    self.draft = draft
                    if let selected { self.profiles = selected.0; self.profileIndex = selected.1 }
                }
            } catch { let message = error.localizedDescription
                DispatchQueue.main.async { [weak self] in self?.failed(message) }
            }
        }
    }
    func selectProfile(_ index: Int) {
        guard profiles.indices.contains(index) else { return }
        change("profile", profiles[index])
    }
    func imported(_ profile: JSON) { change("profile", profile) }
    func preference(_ request: JSON) {
        guard loaded, !busy, !closing, !finished else { return }
        busy = true; error = nil
        let color = details["color"], previous = profiles, preferences = preferences
        NativeProjectTask.io.async { [weak self] in
            do {
                let view = try preferences.presets(color: color, request: request)
                let profile = ExportController.matchProfile(view["recipe"]["profile"], in: previous)
                let draft = view["recipe"].isNull ? nil : try preferences.exportDraft(recipe: view["recipe"])
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.busy = false
                    if self.closing { self.finish(nil); return }
                    self.accept(view, draft: draft, profiles: profile.0, selected: profile.1)
                }
            } catch { let message = error.localizedDescription
                DispatchQueue.main.async { [weak self] in self?.failed(message) }
            }
        }
    }
    func preview(_ recipe: JSON) { prepare(recipe, preview: true) }
    func choose(_ recipe: JSON) { prepare(recipe, preview: false) }
    private func prepare(_ recipe: JSON, preview: Bool) {
        guard loaded, !busy, !closing, !finished, let task else { return }
        busy = true; invalidate()
        let preferences = preferences
        NativeProjectTask.io.async { [weak self] in
            do {
                try task.configureExport(recipe)
                if preview { try task.compare() }
                let details = try task.details()
                let images = preview ? try [task.comparison(after: false), task.comparison(after: true)] : []
                let draft = try preferences.exportDraft(recipe: recipe)
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    self.busy = false
                    if self.closing { self.finish(nil); return }
                    self.draft = draft; self.details = details
                    self.previews = images; self.clipped = details["clipped_channels"].uint
                    if !preview { self.finish(task) }
                }
            } catch { let message = error.localizedDescription
                DispatchQueue.main.async { [weak self] in self?.failed(message) }
            }
        }
    }
    func cancel() {
        guard !finished else { return }
        closing = true; task?.cancel()
        if !busy { finish(nil) }
    }
    private func failed(_ message: String) {
        busy = false
        if closing { finish(nil) } else { error = message }
    }
    private func finish(_ result: NativeProjectTask?) {
        guard !finished else { return }
        finished = true; task = nil; completion(result, recipe, destination, details["color"])
    }
}
