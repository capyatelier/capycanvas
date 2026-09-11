#!/usr/bin/env swift
// Capture the frontmost Capy Canvas window directly, without UI event automation.
import AppKit

enum CaptureFailure: Error { case missingWindow, captureFailed, invalidImage, notFrontmost }
let directory = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "artifacts/ui/parity/mac")
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
guard let window = windows.first(where: {
    ($0[kCGWindowOwnerName as String] as? String) == "CapyCanvas-Mac" &&
    ($0[kCGWindowLayer as String] as? Int) == 0 &&
    (($0[kCGWindowBounds as String] as? [String: Double])?["Width"] ?? 0) > 100
}),
    let bounds = window[kCGWindowBounds as String] as? [String: Double] else {
    fputs("Open the native Mac app before capturing its editor window.\n", stderr)
    throw CaptureFailure.missingWindow
}
guard NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "art.capycanvas.apple.mac" else {
    fputs("Bring the native Mac editor to the front before capturing.\n", stderr)
    throw CaptureFailure.notFrontmost
}
let output = directory.appendingPathComponent("native-mac.png")
let capture = Process()
capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
// Capture the screen inside these exact window bounds. Window-only captures
// contain transparent rounded corners and cannot be compared as opaque images.
// Keep the editor in front and unobscured; preserve system corners in the diff.
let rectangle = ["X", "Y", "Width", "Height"].map { String(Int(bounds[$0] ?? 0)) }.joined(separator: ",")
capture.arguments = ["-x", "-R", rectangle, output.path]
try capture.run(); capture.waitUntilExit()
guard capture.terminationStatus == 0 else { throw CaptureFailure.captureFailed }
guard let bitmap = NSBitmapImageRep(data: try Data(contentsOf: output)),
    let width = bounds["Width"], let height = bounds["Height"], width > 0, height > 0 else {
    throw CaptureFailure.invalidImage
}
let metadata: [String: Any] = ["capture": output.lastPathComponent, "method": "screen-window-bounds",
    "logicalWidth": width, "logicalHeight": height,
    "pixelWidth": bitmap.pixelsWide, "pixelHeight": bitmap.pixelsHigh,
    "scaleX": Double(bitmap.pixelsWide) / width, "scaleY": Double(bitmap.pixelsHigh) / height,
    "osVersion": ProcessInfo.processInfo.operatingSystemVersionString]
let json = try JSONSerialization.data(withJSONObject: metadata, options: [.prettyPrinted, .sortedKeys])
try json.write(to: directory.appendingPathComponent("native-mac.json"))
print(String(decoding: json, as: UTF8.self))
