import UIKit

@MainActor final class PlatformBatterySource: BatterySource {
    private var observers: [NSObjectProtocol] = []
    private var previousMonitoring: Bool?
    private var changed: ((DeviceBattery?) -> Void)?

    func start(_ changed: @escaping (DeviceBattery?) -> Void) {
        stop()
        self.changed = changed
        previousMonitoring = UIDevice.current.isBatteryMonitoringEnabled
        UIDevice.current.isBatteryMonitoringEnabled = true
        for name in [UIDevice.batteryLevelDidChangeNotification, UIDevice.batteryStateDidChangeNotification] {
            observers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.refresh() }
            })
        }
        refresh()
    }
    func stop() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll(); changed = nil
        if let previousMonitoring { UIDevice.current.isBatteryMonitoringEnabled = previousMonitoring }
        previousMonitoring = nil
    }
    private func refresh() {
        let device = UIDevice.current
        guard device.batteryState != .unknown else { changed?(nil); return }
        changed?(DeviceBattery(level: Double(device.batteryLevel),
            charging: device.batteryState == .charging || device.batteryState == .full))
    }
}
