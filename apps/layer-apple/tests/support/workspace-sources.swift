extension WorkspacePresentation {
    var sources: [String: WorkspaceSource] {
        sourceInstances.values.reduce(into: [:]) { result, source in
            if let current = result[source.item], current.layer > source.layer
                || current.layer == source.layer && current.bounds.width * current.bounds.height >= source.bounds.width * source.bounds.height { return }
            result[source.item] = source
        }
    }
}
