import SwiftUI

/// The shared catalog supplies dimensions, limits, labels and defaults. This
/// native sheet collects a choice; Rust validates it again before allocation.
struct NewDrawingForm: View {
    let spec: JSON
    let completion: ([UInt32]?) -> Void
    @State private var width: String
    @State private var height: String
    init(spec: JSON, completion: @escaping ([UInt32]?) -> Void) {
        self.spec = spec; self.completion = completion
        _width = State(initialValue: String(spec["extent"][0].uint))
        _height = State(initialValue: String(spec["extent"][1].uint))
    }
    private var extent: [UInt32]? {
        guard let width = UInt32(width.trimmingCharacters(in: .whitespacesAndNewlines)),
            let height = UInt32(height.trimmingCharacters(in: .whitespacesAndNewlines)),
            [width, height].allSatisfy({ $0 >= spec["minimum"].uint && $0 <= spec["maximum"].uint }) else { return nil }
        return [width, height]
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(spec["title"].string).font(.headline)
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 12) {
                GridRow {
                    Text(spec["labels"][0].string)
                    dimension($width, id: "new-document-width")
                }
                GridRow {
                    Text(spec["labels"][1].string)
                    dimension($height, id: "new-document-height")
                }
            }
            HStack {
                Spacer()
                Button(spec["cancel"].string, role: .cancel) { completion(nil) }
                    .keyboardShortcut(.cancelAction).accessibilityIdentifier("new-document-cancel")
                Button(spec["accept"].string) { if let extent { completion(extent) } }
                    .keyboardShortcut(.defaultAction).disabled(extent == nil)
                    .accessibilityIdentifier("new-document-create")
            }
        }.padding(24).frame(minWidth: 320, idealWidth: 360, maxWidth: 440)
            .presentationDetents([.height(220)])
    }
    private func dimension(_ value: Binding<String>, id: String) -> some View {
        TextField("", text: value).textFieldStyle(.roundedBorder).accessibilityIdentifier(id)
            #if os(iOS)
            .keyboardType(.numberPad)
            #endif
    }
}
