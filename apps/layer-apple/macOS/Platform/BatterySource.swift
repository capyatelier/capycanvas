import Foundation
import IOKit.ps

@MainActor final class PlatformBatterySource: BatterySource {
    private var notificationSource: CFRunLoopSource?
    private var changed: ((DeviceBattery?) -> Void)?
    private var generation: UInt64 = 0

    func start(_ changed: @escaping (DeviceBattery?) -> Void) {
        stop()
        self.changed = changed
        notificationSource = IOPSNotificationCreateRunLoopSource({ context in
            guard let context else { return }
            MainActor.assumeIsolated {
                Unmanaged<PlatformBatterySource>.fromOpaque(context).takeUnretainedValue().refresh()
            }
        }, Unmanaged.passUnretained(self).toOpaque())?.takeRetainedValue()
        if let notificationSource { CFRunLoopAddSource(CFRunLoopGetMain(), notificationSource, .commonModes) }
        refresh()
    }
    func stop() {
        generation &+= 1
        if let notificationSource { CFRunLoopSourceInvalidate(notificationSource) }
        notificationSource = nil; changed = nil
    }
    private func refresh() {
        let generation = generation
        // Power-service IPC is not canvas or UI work. A late reply cannot update
        // a hidden header or a newly started subscription.
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let battery = Self.read()
            DispatchQueue.main.async {
                guard let self, self.generation == generation else { return }
                self.changed?(battery)
            }
        }
    }
    nonisolated static func read() -> DeviceBattery? {
        guard let info = IOPSCopyPowerSourcesInfo()?.takeRetainedValue(),
            let sources = IOPSCopyPowerSourcesList(info)?.takeRetainedValue() as? [CFTypeRef] else { return nil }
        for source in sources {
            guard let value = IOPSGetPowerSourceDescription(info, source)?.takeUnretainedValue() as? [String: Any],
                value[kIOPSTypeKey] as? String == kIOPSInternalBatteryType,
                value[kIOPSIsPresentKey] as? Bool == true,
                let current = value[kIOPSCurrentCapacityKey] as? NSNumber,
                let maximum = value[kIOPSMaxCapacityKey] as? NSNumber, maximum.doubleValue > 0 else { continue }
            return DeviceBattery(level: current.doubleValue / maximum.doubleValue,
                charging: value[kIOPSIsChargingKey] as? Bool == true || value[kIOPSIsChargedKey] as? Bool == true,
                low: IOPSGetBatteryWarningLevel() != kIOPSLowBatteryWarningNone)
        }
        // A desktop, missing source, or invalid reading keeps the clock and
        // omits the battery. Never present a made-up 100% value.
        return nil
    }
}
