/// Menu models are requested on activation, on the editor's serial owner.
/// Native hosts retire late responses when their source or document changes.
typealias AppleContextMenuRequest = @MainActor (@escaping @MainActor (AppleContextMenu?) -> Void) -> Void
