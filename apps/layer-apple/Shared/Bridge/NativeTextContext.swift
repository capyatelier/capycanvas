import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

private struct NativeCopyKey: EnvironmentKey { static let defaultValue = JSON() }
private struct CommonCopyKey: EnvironmentKey { static let defaultValue = JSON() }
private struct ProfileCopyKey: EnvironmentKey { static let defaultValue = JSON() }
private struct InterfaceLanguageKey: EnvironmentKey {
    static let defaultValue = "en"
}
extension EnvironmentValues {
    var capyNativeCopy: JSON {
        get { self[NativeCopyKey.self] }
        set { self[NativeCopyKey.self] = newValue }
    }
    var capyProfileCopy: JSON {
        get { self[ProfileCopyKey.self] }
        set { self[ProfileCopyKey.self] = newValue }
    }
    var capyCommonCopy: JSON {
        get { self[CommonCopyKey.self] }
        set { self[CommonCopyKey.self] = newValue }
    }
    var capyInterfaceLanguage: String {
        get { self[InterfaceLanguageKey.self] }
        set { self[InterfaceLanguageKey.self] = newValue }
    }
}
extension KeyPress {
    var editorFunctionKey: Bool {
        characters.unicodeScalars.count == 1 && (0xF704...0xF71B).contains(characters.unicodeScalars.first?.value ?? 0)
    }
    var editorKey: String {
        let names: [KeyEquivalent: String] = [.upArrow: "arrowup", .downArrow: "arrowdown", .leftArrow: "arrowleft",
            .rightArrow: "arrowright", .home: "home", .end: "end", .pageUp: "pageup", .pageDown: "pagedown",
            .delete: "backspace", .deleteForward: "delete", KeyEquivalent("\u{F727}"): "insert", KeyEquivalent("\u{F746}"): "insert",
            .return: "enter", .tab: "tab", .space: " ", .escape: "escape"]
        if let name = names[key] { return name }
        if editorFunctionKey, let scalar = characters.unicodeScalars.first { return "f\(scalar.value - 0xF703)" }
        return characters
    }
}
@MainActor enum NativeTextContext {
    nonisolated static func shortcutKeyIs(_ key: String, latin: String?, _ expected: String) -> Bool {
        key.withCString { key in
            (latin ?? "").withCString { latin in
                expected.withCString { capy_apple_shortcut_key_is(key, latin, $0) }
            }
        }
    }
    static func numericLabels(_ label: String, language: String) -> JSON {
        do {
            let text = try JSON(["language": language, "request": ["label": label]]).encoded()
            guard let response = text.withCString({ takeNativeText(capy_apple_numeric_labels($0)) }) else { return JSON() }

            return try JSON.decode(response)
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    static func caption(_ request: [String: Any], language: String) -> String {
        do {
            let text = try JSON(["language": language, "request": request]).encoded()
            guard let response = text.withCString({ takeNativeText(capy_apple_native_caption($0)) }) else { return "" }

            return try JSON.decode(response)["text"].string
        } catch { return error.localizedDescription }
    }
    static var inputBusy: Bool {
        if composing { return true }
        #if os(macOS)
        return NSEvent.pressedMouseButtons != 0
        #else
        return UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).contains { capturedInput(in: $0) }
        #endif
    }
    #if os(iOS)
    private static func capturedInput(in view: UIView) -> Bool {
        if (view as? UIControl)?.isTracking == true { return true }
        if view.gestureRecognizers?.contains(where: { $0.state == .began || $0.state == .changed }) == true { return true }
        return view.subviews.contains { capturedInput(in: $0) }
    }
    #endif
    static var composing: Bool {
        #if os(macOS)
        return (NSApp.keyWindow?.firstResponder as? NSTextInputClient)?.hasMarkedText() == true
        #else
        return UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).contains { markedText(in: $0) }
        #endif
    }
    #if os(iOS)
    private static func markedText(in view: UIView) -> Bool {
        if view.isFirstResponder, let input = view as? UITextInput { return input.markedTextRange != nil }
        return view.subviews.contains { markedText(in: $0) }
    }
    #endif
}
