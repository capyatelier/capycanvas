import SwiftUI

struct ProofIndicator: View {
    @ObservedObject var model: ProofController
    let palette: EditorPalette
    var body: some View {
        if !model.status.isEmpty {
            Text(model.status).lineLimit(1).truncationMode(.middle)
                .padding(.horizontal, 8).padding(.vertical, 4)
                .background(palette["bg"], in: Capsule())
                .help(model.error ?? model.status).allowsHitTesting(false)
                .accessibilityIdentifier("proof-status")
        }
    }
}

struct ProofPresentation: ViewModifier {
    @ObservedObject var model: ProofController
    @Environment(\.scenePhase) private var phase
    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(get: { model.setupID != nil }, set: { if !$0 { model.close() } })) {
            Group {
                if model.form.isNull {
                    VStack(spacing: 16) { ProgressView("Opening Proof Setup…"); Button("Cancel") { model.close() } }.padding(24)
                } else { ProofForm(model: model, form: model.form) }
            }.interactiveDismissDisabled(model.committing).modifier(EditorPopupPresentation())
        }
        .onAppear { model.setPaused(phase == .background) }
        .onChange(of: phase) { _, phase in model.setPaused(phase == .background) }
        .onDisappear { model.setPaused(true) }
    }
}

struct ProofForm: View {
    @ObservedObject var model: ProofController
    let form: JSON
    @State private var recipe: JSON
    @State private var profiles: [JSON]
    @State private var selection: String
    @State private var saved: [JSON] = []
    @State private var importing = false
    init(model: ProofController, form: JSON) {
        self.model = model; self.form = form
        let original = form["document_profile"]
        let values = (original.isNull ? [] : [original]) + form["profiles"].array
        _profiles = State(initialValue: values)
        _recipe = State(initialValue: form["recipe"])
        _selection = State(initialValue: String(values.firstIndex {
            SnapshotProjection.equal($0["profile"].raw, form["recipe"]["profile"].raw)
        } ?? 0))
    }
    private var intent: Binding<String> {
        Binding(get: { recipe["conversion"]["intent"].string }, set: {
            var conversion = recipe["conversion"].replacing("intent", with: JSON($0))
            if $0 == "AbsoluteColorimetric" { conversion = conversion.replacing("black_point_compensation", with: JSON(false)) }
            recipe = recipe.replacing("conversion", with: conversion)
        })
    }
    private var simulation: Binding<Int> {
        Binding(get: { recipe["simulate_paper"].bool ? 2 : recipe["simulate_black_ink"].bool ? 1 : 0 }, set: {
            recipe = recipe.replacing("simulate_paper", with: JSON($0 == 2))
                .replacing("simulate_black_ink", with: JSON($0 != 0))
        })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Proof Setup").font(.headline)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Preview how colors will look in print.")
                    FormPicker("Proof profile", selection: Binding(get: { selection }, set: { select($0) })) {
                        if !form["document_profile"].isNull {
                            Section("Document Profile") { Text(profiles[0]["name"].string).tag("0") }
                        }
                        Section("Saved Profiles") {
                            ForEach(saved.indices, id: \.self) { i in
                                Text(saved[i]["name"].string).tag("saved:" + saved[i]["id"].string)
                            }
                        }
                        Section("Standard Color Spaces") {
                            let start = form["document_profile"].isNull ? 0 : 1
                            ForEach(start..<start + form["profiles"].array.count, id: \.self) { i in Text(profiles[i]["name"].string).tag(String(i)) }
                        }
                        if profiles.count > form["profiles"].array.count + (form["document_profile"].isNull ? 0 : 1) {
                            Section("Selected Profiles") {
                                let start = form["profiles"].array.count + (form["document_profile"].isNull ? 0 : 1)
                                ForEach(start..<profiles.count, id: \.self) { i in Text(profiles[i]["name"].string).tag(String(i)) }
                            }
                        }
                    }.accessibilityIdentifier("proof-profile")
                    ProfileChooserButtons(preferences: model.preferences, busy: $importing, onLibraryDismiss: reloadSaved) { profile in
                        choose(profile); reloadSaved()
                    }
                    FormPicker("Rendering intent", selection: intent) {
                        Text("Relative colorimetric").tag("RelativeColorimetric")
                        Text("Perceptual").tag("Perceptual")
                        Text("Saturation").tag("Saturation")
                        Text("Absolute colorimetric").tag("AbsoluteColorimetric")
                    }.accessibilityIdentifier("proof-intent")
                    Toggle("Black point compensation", isOn: Binding(get: { recipe["conversion"]["black_point_compensation"].bool }, set: {
                        recipe = recipe.replacing("conversion", with: recipe["conversion"].replacing("black_point_compensation", with: JSON($0)))
                    })).disabled(intent.wrappedValue == "AbsoluteColorimetric").accessibilityIdentifier("proof-bpc")
                    FormPicker("Print simulation", selection: simulation) {
                        Text("Colors only").tag(0); Text("Black ink").tag(1); Text("Paper and ink").tag(2)
                    }.accessibilityIdentifier("proof-simulation")
                }.frame(maxWidth: .infinity, alignment: .leading).disabled(model.busy || importing)
            }
            if model.busy { ProgressView(model.committing ? "Applying proof…" : "Preparing preview…") }
            if let error = model.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Button("Cancel") { model.close() }.keyboardShortcut(.cancelAction).disabled(model.committing)
                Spacer()
                Button("Apply") { model.apply(recipe) }.keyboardShortcut(.defaultAction)
                    .disabled(model.busy || importing).accessibilityIdentifier("proof-apply")
            }
        }.padding(24).frame(minWidth: 340, idealWidth: 500, maxWidth: 620, minHeight: 300, idealHeight: 460, maxHeight: 720)
            .task { reloadSaved() }
    }
    private func select(_ value: String) {
        if value.hasPrefix("saved:") {
            importing = true
            let preferences = model.preferences
            NativeProjectTask.io.async {
                let result = Result { try preferences.profile(String(value.dropFirst(6))) }
                DispatchQueue.main.async {
                    importing = false
                    switch result {
                    case .success(let profile):
                        selection = value
                        recipe = recipe.replacing("name", with: profile["name"]).replacing("profile", with: profile["profile"])
                    case .failure(let failure): model.error = failure.localizedDescription
                    }
                }
            }
            return
        }
        guard let index = Int(value), profiles.indices.contains(index) else { return }
        selection = value
        recipe = recipe.replacing("name", with: profiles[index]["name"])
            .replacing("profile", with: profiles[index]["profile"])
    }
    private func choose(_ profile: JSON) {
        let base = form["profiles"].array.count + (form["document_profile"].isNull ? 0 : 1)
        profiles = Array(profiles.prefix(base)) + [profile]
        select(String(base))
    }
    private func reloadSaved() {
        let preferences = model.preferences
        NativeProjectTask.io.async {
            let result = Result { try preferences.profiles() }
            DispatchQueue.main.async {
                switch result {
                case .success(let values):
                    saved = values.filter { $0["visible"].bool && $0["issue"].isNull }
                    if selection.hasPrefix("saved:") && !saved.contains(where: { "saved:" + $0["id"].string == selection }) {
                        choose(recipe)
                    }
                case .failure(let failure): model.error = failure.localizedDescription
                }
            }
        }
    }
}
