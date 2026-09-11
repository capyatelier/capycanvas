import Foundation
import Combine

struct DeviceBattery: Equatable, Sendable {
    let percent: Int
    let charging: Bool
    let low: Bool

    init?(level: Double, charging: Bool, low: Bool? = nil) {
        guard level.isFinite, (0...1).contains(level) else { return nil }
        percent = Int((level * 100).rounded())
        self.charging = charging
        self.low = low ?? (percent <= 15)
    }
    var description: String {
        "Battery \(percent.formatted())%" + (charging ? ", charging" : low ? ", low" : "")
    }
}

@MainActor protocol BatterySource: AnyObject {
    func start(_ changed: @escaping (DeviceBattery?) -> Void)
    func stop()
}

/// One process-wide subscription shared by visible editor headers. Clock and
/// power changes never enter the Rust owner or wake the drawing display link.
@MainActor final class SystemStatus: ObservableObject {
    static let shared = SystemStatus(source: PlatformBatterySource())
    @Published private(set) var time = ""
    @Published private(set) var battery: DeviceBattery?
    private let source: any BatterySource
    private let notifications: NotificationCenter
    private let now: () -> Date
    private let formatter = DateFormatter()
    private var observers: [NSObjectProtocol] = []
    private var consumers: Set<UUID> = []
    private var generation: UInt64 = 0
    private var timer: Timer?

    init(source: any BatterySource, notifications: NotificationCenter = .default, now: @escaping () -> Date = Date.init) {
        self.source = source; self.notifications = notifications; self.now = now
        updateClock()
    }
    deinit {
        timer?.invalidate()
        for observer in observers { notifications.removeObserver(observer) }
        let source = source
        Task { @MainActor in source.stop() }
    }
    static func visible(policy: String, fullscreen: Bool) -> Bool {
        policy == "always" || (policy != "never" && fullscreen)
    }
    static func nextMinute(after date: Date) -> Date {
        Date(timeIntervalSince1970: (floor(date.timeIntervalSince1970 / 60) + 1) * 60 + 0.02)
    }
    func acquire() -> UUID {
        let id = UUID()
        consumers.insert(id)
        guard consumers.count == 1 else { return id }
        generation &+= 1
        let generation = generation
        source.start { [weak self] value in
            guard let self, self.generation == generation, !self.consumers.isEmpty else { return }
            if self.battery != value { self.battery = value }
        }
        for name in [NSLocale.currentLocaleDidChangeNotification, .NSSystemTimeZoneDidChange, .NSSystemClockDidChange] {
            observers.append(notifications.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.updateClock() }
            })
        }
        updateClock()
        return id
    }
    func release(_ id: UUID) {
        guard consumers.remove(id) != nil, consumers.isEmpty else { return }
        generation &+= 1
        timer?.invalidate(); timer = nil
        for observer in observers { notifications.removeObserver(observer) }
        observers.removeAll()
        source.stop()
        battery = nil
    }
    private func updateClock() {
        // Recreate the localized format after locale, time-zone or 12/24-hour
        // changes. DateFormatter's short style honors the system preference.
        formatter.locale = .autoupdatingCurrent
        formatter.timeZone = .autoupdatingCurrent
        formatter.dateStyle = .none; formatter.timeStyle = .short
        let date = now(), text = formatter.string(from: date)
        if text != time { time = text }
        timer?.invalidate(); timer = nil
        guard !consumers.isEmpty else { return }
        let next = Timer(fire: Self.nextMinute(after: date), interval: 0, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.updateClock() }
        }
        next.tolerance = 0.1
        timer = next
        RunLoop.main.add(next, forMode: .common)
    }
}
