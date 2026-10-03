import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

private struct NativeCopyKey: EnvironmentKey { static let defaultValue = JSON() }
private struct CommonCopyKey: EnvironmentKey { static let defaultValue = JSON() }
private struct InterfaceLanguageKey: EnvironmentKey {
    static let defaultValue = "en"
}
extension EnvironmentValues {
    var capyNativeCopy: JSON {
        get { self[NativeCopyKey.self] }
        set { self[NativeCopyKey.self] = newValue }
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
@MainActor enum NativeTextContext {
    static func appearance(_ options: JSON, language: String) -> JSON {
        do {
            let text = try JSON(["language": language, "request": options.raw]).encoded()
            guard let response = text.withCString({ capy_apple_document_appearance($0) }) else { return JSON() }
            defer { capy_apple_string_free(response) }
            return try JSON.decode(String(cString: response))
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    static func numericLabels(_ label: String, language: String) -> JSON {
        do {
            let text = try JSON(["language": language, "request": ["label": label]]).encoded()
            guard let response = text.withCString({ capy_apple_numeric_labels($0) }) else { return JSON() }
            defer { capy_apple_string_free(response) }
            return try JSON.decode(String(cString: response))
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    static func caption(_ request: [String: Any], language: String) -> String {
        do {
            let text = try JSON(["language": language, "request": request]).encoded()
            guard let response = text.withCString({ capy_apple_native_caption($0) }) else { return "" }
            defer { capy_apple_string_free(response) }
            return try JSON.decode(String(cString: response))["text"].string
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
