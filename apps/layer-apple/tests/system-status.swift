// Direct lifecycle and formatting checks; no windows, menus or renderer.
import Foundation

@MainActor final class BatteryFixture: BatterySource {
    var starts = 0, stops = 0
    var callbacks: [(DeviceBattery?) -> Void] = []
    func start(_ changed: @escaping (DeviceBattery?) -> Void) { starts += 1; callbacks.append(changed) }
    func stop() { stops += 1 }
}

@main struct SystemStatusChecks {
    @MainActor static func drain() async {
        await withCheckedContinuation { continuation in DispatchQueue.main.async { continuation.resume() } }
    }
    @MainActor static func main() async {
        for invalid in [-1.0, 1.01, Double.nan, Double.infinity] {
            precondition(DeviceBattery(level: invalid, charging: false) == nil)
        }
        let low = DeviceBattery(level: 0.15, charging: false)!
        precondition(low.low && low.percent == 15 && low.description.hasSuffix(", low"))
        precondition(DeviceBattery(level: 0, charging: false)?.percent == 0)
        precondition(DeviceBattery(level: 0.16, charging: false)?.low == false)
        precondition(DeviceBattery(level: 1, charging: true)?.percent == 100)
        precondition(DeviceBattery(level: 0.15, charging: true)!.description.hasSuffix(", charging"))
        precondition(DeviceBattery(level: 0.4, charging: false, low: true)!.low, "Honor a native low-battery warning")
        for fullscreen in [false, true] {
            precondition(SystemStatus.visible(policy: "always", fullscreen: fullscreen))
            precondition(!SystemStatus.visible(policy: "never", fullscreen: fullscreen))
            precondition(SystemStatus.visible(policy: "fullscreen", fullscreen: fullscreen) == fullscreen)
        }
        let before = Date(timeIntervalSince1970: 1234.5)
        precondition(abs(SystemStatus.nextMinute(after: before).timeIntervalSince1970 - 1260.02) < 0.001)
        precondition(SystemStatus.nextMinute(after: Date(timeIntervalSince1970: 1260)).timeIntervalSince1970 > 1319)

        let source = BatteryFixture(), notifications = NotificationCenter()
        var date = before
        let status = SystemStatus(source: source, notifications: notifications, now: { date })
        let initialTime = status.time
        precondition(source.starts == 0, "A hidden header must not monitor the device")
        let first = status.acquire(), second = status.acquire()
        precondition(source.starts == 1, "Visible windows share one native subscription")
        source.callbacks[0](low)
        precondition(status.battery == low)
        date += 120
        notifications.post(name: .NSSystemClockDidChange, object: nil)
        await drain()
        precondition(status.time != initialTime, "A wall-clock change refreshes immediately")
        status.release(first); status.release(first)
        precondition(source.stops == 0, "Closing one window cannot stop another window's status")
        source.callbacks[0](nil)
        precondition(status.battery == nil && !status.time.isEmpty, "Unknown battery must not hide the clock")
        status.release(second)
        precondition(source.stops == 1)
        let stoppedTime = status.time
        date += 120
        notifications.post(name: .NSSystemClockDidChange, object: nil)
        await drain()
        precondition(status.time == stoppedTime, "Hidden headers have no notification/timer work")
        let replacement = status.acquire()
        precondition(status.time != stoppedTime && source.starts == 2)
        source.callbacks[0](low)
        precondition(status.battery == nil, "A late reply from a retired subscription must be ignored")
        source.callbacks[1](low)
        precondition(status.battery == low)
        status.release(replacement)
        precondition(source.stops == 2 && status.battery == nil)
        print("PASS: clock policy, minute scheduling, native battery normalization, shared window lifetime and late replies")
    }
}
