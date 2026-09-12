import SwiftUI

struct SystemStatusView: View {
    let dark: Bool
    @ObservedObject var status = SystemStatus.shared
    var spacing: CGFloat = 6
    @Environment(\.scenePhase) private var phase
    @State private var subscription: UUID?

    var body: some View {
        HStack(spacing: spacing) {
            Text(status.time).monospacedDigit().lineLimit(1).fixedSize()
                .padding(.horizontal, 6).frame(height: 36)
                .accessibilityIdentifier("system-clock")
                .modifier(HeaderControlMeasurement(id: "system-clock"))
            if let battery = status.battery {
                BatteryIndicator(battery: battery, dark: dark).frame(width: 36, height: 36)
            }
        }
            .onAppear { subscribe() }
            .onChange(of: phase) { _, _ in subscribe() }
            .onDisappear { unsubscribe() }
    }
    private func subscribe() {
        if phase == .background { unsubscribe() }
        else if subscription == nil { subscription = status.acquire() }
    }
    private func unsubscribe() {
        if let subscription { status.release(subscription) }
        subscription = nil
    }
}

/// The browser/Android battery shape, percentage and palette, in logical points.
struct BatteryIndicator: View {
    let battery: DeviceBattery
    let dark: Bool
    private var colors: (Color, Color, Color) {
        if battery.charging { return (Color(hex: "91b89d"), Color(hex: "c4c9cf"), Color(hex: "13251a")) }
        let fill = battery.low ? (dark ? "bc9996" : "a15d59") : (dark ? "e5e7eb" : "3f4246")
        return (Color(hex: fill), Color(hex: dark ? "a3a8b0" : "707479"), Color(hex: dark ? "202226" : "ffffff"))
    }
    var body: some View {
        Canvas { context, _ in
            let (fill, track, ink) = colors
            let shell = Path(roundedRect: CGRect(x: 0, y: 0, width: 22, height: 14), cornerRadius: 4)
            context.fill(shell, with: .color(track))
            var charged = context
            charged.clip(to: Path(CGRect(x: 0, y: 0, width: 22 * Double(battery.percent) / 100, height: 14)))
            charged.fill(shell, with: .color(fill))
            let percent = Text(battery.percent.formatted()).font(.system(size: battery.percent == 100 ? 10 : 11, weight: .bold)).foregroundStyle(ink)
            context.draw(percent, at: CGPoint(x: battery.charging ? 10.5 : 11, y: 7))
            if battery.charging {
                var bolt = Path()
                bolt.move(to: CGPoint(x: 23, y: 2))
                for point in [CGPoint(x: 18.5, y: 8), CGPoint(x: 21.2, y: 8), CGPoint(x: 20, y: 12),
                    CGPoint(x: 25, y: 5.9), CGPoint(x: 22.7, y: 5.9), CGPoint(x: 24.1, y: 2)] { bolt.addLine(to: point) }
                bolt.closeSubpath()
                context.stroke(bolt, with: .color(dark ? Color(hex: "202226") : track),
                    style: StrokeStyle(lineWidth: 1.75, lineJoin: .round))
                context.fill(bolt, with: .color(dark ? Color(hex: "e5e7eb") : ink))
            } else {
                context.fill(Path(roundedRect: CGRect(x: 23, y: 4, width: 2, height: 6), cornerRadius: 1), with: .color(track))
            }
        }.frame(width: 26, height: 14)
            .accessibilityElement().accessibilityLabel(battery.description).help(battery.description)
            .accessibilityIdentifier("system-battery")
            .modifier(HeaderControlMeasurement(id: "system-battery"))
    }
}
